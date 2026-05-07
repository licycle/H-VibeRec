#!/usr/bin/env python3
from __future__ import annotations

import inspect
import json
import os
import subprocess
import sys
import time
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Sequence

ENGINE_NAME = "FunASR-Workflow"
DEFAULT_PROFILE = "meeting_full"
DICTATION_PROFILE = "dictation"
DEFAULT_ASR_MODEL = "paraformer-zh"
DEFAULT_VAD_MODEL_NAME = "fsmn-vad"
DEFAULT_SPEAKER_MODEL_NAME = "cam++"
DEFAULT_SPEAKER_MODEL_DIR = "campplus"
DEFAULT_PUNC_MODEL_NAME = "ct-punc-c"
DEFAULT_PUNC_MODEL_DIR = "ct-punc-c"
DEFAULT_AUXILIARY_DIR = ".voice_vibe_aux"

MODELSCOPE_MODEL_ALIASES = {
    DEFAULT_ASR_MODEL: "iic/speech_seaco_paraformer_large_asr_nat-zh-cn-16k-common-vocab8404-pytorch",
    DEFAULT_VAD_MODEL_NAME: "iic/speech_fsmn_vad_zh-cn-16k-common-pytorch",
    DEFAULT_SPEAKER_MODEL_NAME: "iic/speech_campplus_sv_zh-cn_16k-common",
    DEFAULT_SPEAKER_MODEL_DIR: "iic/speech_campplus_sv_zh-cn_16k-common",
    DEFAULT_PUNC_MODEL_NAME: "iic/punc_ct-transformer_zh-cn-common-vocab272727-pytorch",
}

HUGGINGFACE_MODEL_ALIASES = {
    DEFAULT_ASR_MODEL: "funasr/paraformer-zh",
    DEFAULT_VAD_MODEL_NAME: "funasr/fsmn-vad",
    DEFAULT_SPEAKER_MODEL_NAME: "funasr/campplus",
    DEFAULT_SPEAKER_MODEL_DIR: "funasr/campplus",
    # FunASR exposes ct-punc-c primarily through ModelScope. The Hugging Face
    # fallback keeps the HF source usable while ModelScope remains the default.
    DEFAULT_PUNC_MODEL_NAME: "funasr/ct-punc",
}

DEFAULT_FUNASR_MODEL_REVISION = "v2.0.4"
FUNASR_MODEL_REVISIONS = {
    DEFAULT_ASR_MODEL: DEFAULT_FUNASR_MODEL_REVISION,
    DEFAULT_VAD_MODEL_NAME: DEFAULT_FUNASR_MODEL_REVISION,
    DEFAULT_PUNC_MODEL_NAME: DEFAULT_FUNASR_MODEL_REVISION,
    MODELSCOPE_MODEL_ALIASES[DEFAULT_ASR_MODEL]: DEFAULT_FUNASR_MODEL_REVISION,
    MODELSCOPE_MODEL_ALIASES[DEFAULT_VAD_MODEL_NAME]: DEFAULT_FUNASR_MODEL_REVISION,
    MODELSCOPE_MODEL_ALIASES[DEFAULT_PUNC_MODEL_NAME]: DEFAULT_FUNASR_MODEL_REVISION,
}


class SidecarError(RuntimeError):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


_WORKFLOW_MODEL_CACHE: Dict[str, Any] = {
    "key": None,
    "model": None,
}


@dataclass
class TranscriptSegment:
    id: int
    start_ms: int
    end_ms: int
    speaker_label: str
    speaker_index: int
    text: str
    confidence: float | None = None


def emit(payload: Dict[str, Any]) -> None:
    print(json.dumps(payload, ensure_ascii=False))
    sys.stdout.flush()


def read_request() -> Dict[str, Any]:
    raw = sys.stdin.readline()
    if not raw:
        raise RuntimeError("No request received")
    return json.loads(raw)


def response_ok(req_id: str, result: Dict[str, Any]) -> Dict[str, Any]:
    return {"id": req_id, "ok": True, "result": result}


def response_error(req_id: str, code: str, message: str, details: Dict[str, Any] | None = None) -> Dict[str, Any]:
    payload = {"id": req_id, "ok": False, "error": {"code": code, "message": message}}
    if details is not None:
        payload["error"]["details"] = details
    return payload


def ffmpeg_exists(ffmpeg_path: str) -> bool:
    try:
        subprocess.run(
            [ffmpeg_path, "-version"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return True
    except Exception:
        return False


def prepend_executable_dir_to_path(executable_path: str) -> None:
    executable_dir = Path(executable_path).parent
    if str(executable_dir) in ("", "."):
        return

    bin_dir = str(executable_dir)
    current_path = os.environ.get("PATH", "")
    path_parts = current_path.split(os.pathsep) if current_path else []
    if bin_dir not in path_parts:
        os.environ["PATH"] = os.pathsep.join([bin_dir, *path_parts]) if path_parts else bin_dir


def normalize_audio(ffmpeg_path: str, input_path: str, output_path: str) -> None:
    Path(output_path).parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            ffmpeg_path,
            "-y",
            "-i",
            input_path,
            "-ar",
            "16000",
            "-ac",
            "1",
            "-acodec",
            "pcm_s16le",
            "-f",
            "wav",
            output_path,
        ],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def wav_duration_seconds(audio_path: str) -> float | None:
    try:
        with wave.open(audio_path, "rb") as wav_file:
            sample_rate = wav_file.getframerate()
            if sample_rate <= 0:
                return None
            return wav_file.getnframes() / float(sample_rate)
    except Exception:
        return None


def download_model(repo: str, target_dir: str, source: str = "modelscope") -> str:
    Path(target_dir).mkdir(parents=True, exist_ok=True)
    resolved_repo = resolve_model_alias(repo, source)
    revision = model_revision_for(repo, source)
    if source == "huggingface":
        return download_huggingface_model(resolved_repo, target_dir, revision)
    if source == "modelscope":
        return download_modelscope_model(resolved_repo, target_dir, revision)
    raise ValueError(f"Unsupported model source: {source}")


def resolve_model_alias(repo: str, source: str) -> str:
    repo = (repo or "").strip() or DEFAULT_ASR_MODEL
    if source == "huggingface":
        return HUGGINGFACE_MODEL_ALIASES.get(repo, repo)
    if source == "modelscope":
        return MODELSCOPE_MODEL_ALIASES.get(repo, repo)
    return repo


def model_revision_for(repo: str, source: str) -> str | None:
    requested_repo = (repo or "").strip() or DEFAULT_ASR_MODEL
    resolved_repo = resolve_model_alias(requested_repo, source)
    if source == "huggingface":
        return None
    return FUNASR_MODEL_REVISIONS.get(requested_repo) or FUNASR_MODEL_REVISIONS.get(resolved_repo)


def proxy_env_value(key: str) -> str | None:
    value = os.environ.get(key) or os.environ.get(key.upper())
    if value and value.strip():
        return value.strip()
    return None


def mask_proxy(proxy: str) -> str:
    if "://" not in proxy:
        return "set"
    scheme, rest = proxy.split("://", 1)
    host = rest.rsplit("@", 1)[-1].split("/", 1)[0]
    return f"{scheme}://{host}"


def download_proxies() -> Dict[str, str] | None:
    http_proxy = proxy_env_value("http_proxy")
    https_proxy = proxy_env_value("https_proxy") or http_proxy
    if not http_proxy and not https_proxy:
        return None
    proxies: Dict[str, str] = {}
    if http_proxy:
        proxies["http"] = http_proxy
    if https_proxy:
        proxies["https"] = https_proxy
    return proxies


def log_download_proxy(source: str) -> None:
    proxies = download_proxies()
    all_proxy = proxy_env_value("all_proxy")
    if not proxies and not all_proxy:
        sys.stderr.write(f"[funasr_workflow] {source} download proxy: unset\n")
        sys.stderr.flush()
        return
    http_proxy = proxies.get("http") if proxies else None
    https_proxy = proxies.get("https") if proxies else None
    sys.stderr.write(
        "[funasr_workflow] "
        f"{source} download proxy: "
        f"http={mask_proxy(http_proxy) if http_proxy else 'unset'} "
        f"https={mask_proxy(https_proxy) if https_proxy else 'unset'} "
        f"all={mask_proxy(all_proxy) if all_proxy else 'unset'}\n"
    )
    sys.stderr.flush()


def download_huggingface_model(repo: str, target_dir: str, revision: str | None = None) -> str:
    from huggingface_hub import snapshot_download as hf_snapshot_download

    kwargs = {
        "repo_id": repo,
        "local_dir": target_dir,
    }
    log_download_proxy("Hugging Face")
    if revision:
        kwargs["revision"] = revision
    signature = inspect.signature(hf_snapshot_download)
    proxies = download_proxies()
    if proxies and "proxies" in signature.parameters:
        kwargs["proxies"] = proxies
    if "local_dir_use_symlinks" in signature.parameters:
        kwargs["local_dir_use_symlinks"] = False
    local_dir = hf_snapshot_download(**kwargs)
    return str(local_dir)


def download_modelscope_model(repo: str, target_dir: str, revision: str | None = None) -> str:
    try:
        from modelscope import snapshot_download as ms_snapshot_download
    except Exception as exc:
        raise RuntimeError(f"ModelScope SDK is not available: {exc}") from exc

    log_download_proxy("ModelScope")
    kwargs = {
        "model_id": repo,
        "local_dir": target_dir,
    }
    signature = inspect.signature(ms_snapshot_download)
    if revision and "revision" in signature.parameters:
        kwargs["revision"] = revision
    local_dir = ms_snapshot_download(**kwargs)
    return str(local_dir)


def estimate_repo_download_size(repo: str, source: str, revision: str | None = None) -> int:
    if source == "huggingface":
        return estimate_huggingface_repo_size(repo, revision)
    if source == "modelscope":
        return estimate_modelscope_repo_size(repo, revision)
    raise ValueError(f"Unsupported model source: {source}")


def estimate_huggingface_repo_size(repo: str, revision: str | None = None) -> int:
    from huggingface_hub import HfApi

    api = HfApi()
    kwargs: Dict[str, Any] = {
        "repo_id": repo,
        "files_metadata": True,
    }
    if revision:
        kwargs["revision"] = revision
    info = api.model_info(**kwargs)
    total = 0
    missing: List[str] = []
    for sibling in getattr(info, "siblings", []) or []:
        size = getattr(sibling, "size", None)
        lfs = getattr(sibling, "lfs", None)
        if size is None and lfs:
            if isinstance(lfs, dict):
                size = lfs.get("size")
            else:
                size = getattr(lfs, "size", None)
        if size is None:
            missing.append(str(getattr(sibling, "rfilename", "unknown")))
            continue
        total += int(size)
    if missing:
        raise RuntimeError(f"Hugging Face file sizes unavailable for {repo}: {missing}")
    return total


def estimate_modelscope_repo_size(repo: str, revision: str | None = None) -> int:
    from modelscope.hub.api import HubApi

    api = HubApi()
    kwargs: Dict[str, Any] = {
        "model_id": repo,
        "recursive": True,
    }
    if revision:
        kwargs["revision"] = revision
    files = api.get_model_files(**kwargs)
    total = 0
    missing: List[str] = []
    file_count = 0
    for item in files:
        if item.get("Type") == "tree":
            continue
        file_count += 1
        size = item.get("Size")
        if size is None:
            missing.append(str(item.get("Path") or item.get("Name") or "unknown"))
            continue
        total += int(size)
    if missing:
        raise RuntimeError(f"ModelScope file sizes unavailable for {repo}: {missing}")
    if file_count == 0:
        raise RuntimeError(f"ModelScope file list is empty for {repo}")
    return total


def model_download_plan(repo: str, source: str, profile: str) -> List[Dict[str, str | None]]:
    requested_repo = (repo or "").strip() or DEFAULT_ASR_MODEL
    normalized_profile = str(profile or DEFAULT_PROFILE).strip()
    asr = {
        "name": DEFAULT_ASR_MODEL,
        "repo": resolve_model_alias(requested_repo, source),
        "revision": model_revision_for(requested_repo, source),
    }
    vad = {
        "name": DEFAULT_VAD_MODEL_NAME,
        "repo": resolve_model_alias(DEFAULT_VAD_MODEL_NAME, source),
        "revision": model_revision_for(DEFAULT_VAD_MODEL_NAME, source),
    }
    speaker = {
        "name": DEFAULT_SPEAKER_MODEL_NAME,
        "repo": resolve_model_alias(DEFAULT_SPEAKER_MODEL_NAME, source),
        "revision": model_revision_for(DEFAULT_SPEAKER_MODEL_NAME, source),
    }
    punc = {
        "name": DEFAULT_PUNC_MODEL_NAME,
        "repo": resolve_model_alias(DEFAULT_PUNC_MODEL_NAME, source),
        "revision": model_revision_for(DEFAULT_PUNC_MODEL_NAME, source),
    }
    if normalized_profile == "dictation":
        return [asr, punc]
    if normalized_profile == "auxiliary":
        return [vad, speaker, punc]
    if normalized_profile == "punctuation":
        return [punc]
    return [asr, vad, speaker, punc]


def estimate_model_download_size(repo: str, source: str = "modelscope", profile: str = DEFAULT_PROFILE) -> Dict[str, Any]:
    models = []
    total = 0
    for item in model_download_plan(repo, source, profile):
        item_repo = str(item["repo"])
        item_revision = item.get("revision")
        size = estimate_repo_download_size(item_repo, source, str(item_revision) if item_revision else None)
        total += size
        models.append(
            {
                "name": item["name"],
                "repo": item_repo,
                "revision": item_revision,
                "bytes": size,
            }
        )
    return {
        "repo": (repo or "").strip() or DEFAULT_ASR_MODEL,
        "source": source,
        "profile": profile,
        "total_bytes": total,
        "models": models,
    }


def auxiliary_root_for_model(model_path: str) -> str:
    return str(Path(model_path).joinpath(DEFAULT_AUXILIARY_DIR))


def auxiliary_model_paths(auxiliary_root: str) -> Dict[str, str]:
    return {
        "vad_model_path": str(Path(auxiliary_root).joinpath(DEFAULT_VAD_MODEL_NAME)),
        "speaker_model_path": str(Path(auxiliary_root).joinpath(DEFAULT_SPEAKER_MODEL_DIR)),
        "punc_model_path": str(Path(auxiliary_root).joinpath(DEFAULT_PUNC_MODEL_DIR)),
    }


def prepare_auxiliary_models(auxiliary_root: str, source: str = "modelscope") -> Dict[str, Any]:
    paths = auxiliary_model_paths(auxiliary_root)
    Path(auxiliary_root).mkdir(parents=True, exist_ok=True)

    vad_repo = resolve_model_alias(DEFAULT_VAD_MODEL_NAME, source)
    speaker_repo = resolve_model_alias(DEFAULT_SPEAKER_MODEL_NAME, source)
    punc_repo = resolve_model_alias(DEFAULT_PUNC_MODEL_NAME, source)
    vad_revision = model_revision_for(DEFAULT_VAD_MODEL_NAME, source)
    speaker_revision = model_revision_for(DEFAULT_SPEAKER_MODEL_NAME, source)
    punc_revision = model_revision_for(DEFAULT_PUNC_MODEL_NAME, source)

    vad_path = download_model(DEFAULT_VAD_MODEL_NAME, paths["vad_model_path"], source)
    speaker_path = download_model(DEFAULT_SPEAKER_MODEL_NAME, paths["speaker_model_path"], source)
    punc_path = download_model(DEFAULT_PUNC_MODEL_NAME, paths["punc_model_path"], source)

    return {
        "auxiliary_model_root": auxiliary_root,
        "vad_model_path": vad_path,
        "speaker_model_path": speaker_path,
        "punc_model_path": punc_path,
        "auxiliary_models": {
            "vad": {
                "name": DEFAULT_VAD_MODEL_NAME,
                "repo": vad_repo,
                "revision": vad_revision,
                "path": vad_path,
            },
            "speaker": {
                "name": DEFAULT_SPEAKER_MODEL_NAME,
                "repo": speaker_repo,
                "revision": speaker_revision,
                "path": speaker_path,
            },
            "punc": {
                "name": DEFAULT_PUNC_MODEL_NAME,
                "repo": punc_repo,
                "revision": punc_revision,
                "path": punc_path,
            },
        },
    }


def prepare_punctuation_model(auxiliary_root: str, source: str = "modelscope") -> Dict[str, Any]:
    paths = auxiliary_model_paths(auxiliary_root)
    Path(auxiliary_root).mkdir(parents=True, exist_ok=True)

    punc_repo = resolve_model_alias(DEFAULT_PUNC_MODEL_NAME, source)
    punc_revision = model_revision_for(DEFAULT_PUNC_MODEL_NAME, source)
    punc_path = download_model(DEFAULT_PUNC_MODEL_NAME, paths["punc_model_path"], source)

    return {
        "auxiliary_model_root": auxiliary_root,
        "punc_model_path": punc_path,
        "auxiliary_models": {
            "punc": {
                "name": DEFAULT_PUNC_MODEL_NAME,
                "repo": punc_repo,
                "revision": punc_revision,
                "path": punc_path,
            },
        },
    }


def prepare_pipeline_models(repo: str, target_dir: str, source: str = "modelscope") -> Dict[str, Any]:
    requested_repo = (repo or "").strip() or DEFAULT_ASR_MODEL
    model_path = download_model(requested_repo, target_dir, source)
    auxiliary = prepare_auxiliary_models(auxiliary_root_for_model(model_path), source)
    return {
        "repo": requested_repo,
        "resolved_repo": resolve_model_alias(requested_repo, source),
        "model_revision": model_revision_for(requested_repo, source),
        "model_path": model_path,
        "source": source,
        **auxiliary,
    }


def prepare_dictation_models(repo: str, target_dir: str, source: str = "modelscope") -> Dict[str, Any]:
    requested_repo = (repo or "").strip() or DEFAULT_ASR_MODEL
    model_path = download_model(requested_repo, target_dir, source)
    auxiliary = prepare_punctuation_model(auxiliary_root_for_model(model_path), source)
    return {
        "profile": DICTATION_PROFILE,
        "repo": requested_repo,
        "resolved_repo": resolve_model_alias(requested_repo, source),
        "model_revision": model_revision_for(requested_repo, source),
        "model_path": model_path,
        "source": source,
        **auxiliary,
    }


def require_local_model_path(path: str, label: str) -> str:
    if not Path(path).exists():
        raise SidecarError("MODEL_NOT_READY", f"{label} model path not found: {path}")
    return path


def normalize_profile(value: Any) -> str:
    profile = str(value or DEFAULT_PROFILE).strip()
    if profile == DICTATION_PROFILE:
        return DICTATION_PROFILE
    return DEFAULT_PROFILE


def get_gpu_memory_info() -> Dict[str, float] | None:
    try:
        import torch

        if not torch.cuda.is_available():
            return None
        allocated = torch.cuda.memory_allocated() / 1024**3
        cached = torch.cuda.memory_reserved() / 1024**3
        total = torch.cuda.get_device_properties(0).total_memory / 1024**3
        free = total - allocated
        return {
            "allocated": allocated,
            "cached": cached,
            "total": total,
            "free": free,
            "usage_ratio": allocated / total if total > 0 else 0.0,
        }
    except Exception:
        return None


def get_optimized_model_params(audio_file_path: str | None = None) -> Dict[str, int | float]:
    params: Dict[str, int | float] = {
        "batch_size_s": 300,
        "batch_size_threshold_s": 60,
        "max_single_segment_time": 30000,
        "batch_size": 8,
        "ncpu": max(1, min(8, os.cpu_count() or 4)),
    }

    gpu_info = get_gpu_memory_info()
    if gpu_info:
        gpu_usage = gpu_info["usage_ratio"]
        gpu_free = gpu_info["free"]
        if gpu_usage > 0.85:
            params["batch_size_s"] = 150
            params["batch_size_threshold_s"] = 30
            params["max_single_segment_time"] = 15000
            params["batch_size"] = 4
        elif gpu_free < 2.0:
            params["batch_size_s"] = 200
            params["batch_size_threshold_s"] = 45
            params["batch_size"] = 4

    if audio_file_path and os.path.exists(audio_file_path):
        file_size_mb = os.path.getsize(audio_file_path) / 1024 / 1024
        if file_size_mb > 100:
            params["batch_size_s"] = 60
            params["batch_size_threshold_s"] = 30
            params["max_single_segment_time"] = 10000
            params["batch_size"] = 1
        elif file_size_mb > 50:
            params["batch_size_s"] = 120
            params["batch_size_threshold_s"] = 40
            params["max_single_segment_time"] = 20000
            params["batch_size"] = 2

    params["batch_size_s"] = max(60, int(params["batch_size_s"]))
    params["batch_size_threshold_s"] = max(15, int(params["batch_size_threshold_s"]))
    params["max_single_segment_time"] = max(5000, int(params["max_single_segment_time"]))
    params["batch_size"] = max(1, int(params["batch_size"]))
    params["ncpu"] = max(1, int(params["ncpu"]))
    return params


def get_inference_device(use_gpu: bool = True) -> str:
    requested = os.getenv("VOICE_VIBE_DEVICE", "auto").strip().lower()
    if requested == "cpu" or not use_gpu:
        return "cpu"

    try:
        import torch

        cuda_available = torch.cuda.is_available()
        mps_backend = getattr(getattr(torch, "backends", None), "mps", None)
        mps_available = bool(mps_backend and mps_backend.is_available())

        if requested.startswith("cuda"):
            return requested if cuda_available else "cpu"
        if requested == "mps":
            if mps_available:
                os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")
                return "mps"
            return "cpu"
        if requested == "gpu":
            if cuda_available:
                return "cuda"
            if mps_available:
                os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")
                return "mps"
            return "cpu"
        if cuda_available:
            return "cuda"
        if mps_available:
            os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")
            return "mps"
    except Exception:
        pass
    return "cpu"


def make_workflow_model(
    model_path: str,
    vad_model_path: str | None,
    speaker_model_path: str | None,
    punc_model_path: str | None,
    use_gpu: bool,
    audio_file_path: str | None = None,
) -> Any:
    from funasr import AutoModel

    params = get_optimized_model_params(audio_file_path)
    kwargs: Dict[str, Any] = {
        "model": model_path,
        "model_revision": DEFAULT_FUNASR_MODEL_REVISION,
        "disable_update": True,
        "check_latest": False,
        "device": get_inference_device(use_gpu),
        "batch_size": params["batch_size"],
        "ncpu": params["ncpu"],
        "disable_pbar": True,
    }
    if vad_model_path:
        kwargs["vad_model"] = vad_model_path
        kwargs["vad_model_revision"] = DEFAULT_FUNASR_MODEL_REVISION
        kwargs["vad_kwargs"] = {
            "max_single_segment_time": params["max_single_segment_time"],
            "batch_size_s": params["batch_size_s"],
            "batch_size_threshold_s": params["batch_size_threshold_s"],
            "check_latest": False,
            "disable_pbar": True,
        }
    if speaker_model_path:
        kwargs["spk_model"] = speaker_model_path
        kwargs["spk_kwargs"] = {
            "check_latest": False,
            "disable_pbar": True,
        }
    if punc_model_path:
        kwargs["punc_model"] = punc_model_path
        kwargs["punc_model_revision"] = DEFAULT_FUNASR_MODEL_REVISION
        kwargs["punc_kwargs"] = {
            "check_latest": False,
            "disable_pbar": True,
        }
    return AutoModel(**kwargs)


def generate_with_retries(
    model_path: str,
    vad_model_path: str | None,
    speaker_model_path: str | None,
    punc_model_path: str | None,
    use_gpu: bool,
    audio_path: str,
) -> Any:
    max_retries = 3
    last_error: Exception | None = None
    for attempt in range(max_retries):
        try:
            model = make_workflow_model(
                model_path,
                vad_model_path,
                speaker_model_path,
                punc_model_path,
                use_gpu,
                audio_path,
            )
            return model.generate(input=audio_path)
        except Exception as exc:
            last_error = exc
            error_msg = str(exc).lower()
            is_oom = any(
                token in error_msg
                for token in ("out of memory", "oom", "cuda out of memory", "allocation")
            )
            if attempt >= max_retries - 1 or not is_oom:
                break
    raise RuntimeError(f"FunASR workflow inference failed: {last_error}") from last_error


def get_cached_workflow_model(
    model_path: str,
    vad_model_path: str | None,
    speaker_model_path: str | None,
    punc_model_path: str | None,
    use_gpu: bool,
    audio_path: str,
) -> Any:
    params = get_optimized_model_params(audio_path)
    key = json.dumps(
        {
            "model_path": model_path,
            "vad_model_path": vad_model_path,
            "speaker_model_path": speaker_model_path,
            "punc_model_path": punc_model_path,
            "use_gpu": use_gpu,
            "device": get_inference_device(use_gpu),
            "batch_size_s": params["batch_size_s"],
            "batch_size_threshold_s": params["batch_size_threshold_s"],
            "max_single_segment_time": params["max_single_segment_time"],
            "batch_size": params["batch_size"],
            "ncpu": params["ncpu"],
        },
        sort_keys=True,
    )
    if _WORKFLOW_MODEL_CACHE.get("key") == key and _WORKFLOW_MODEL_CACHE.get("model") is not None:
        return _WORKFLOW_MODEL_CACHE["model"]

    _WORKFLOW_MODEL_CACHE["model"] = make_workflow_model(
        model_path,
        vad_model_path,
        speaker_model_path,
        punc_model_path,
        use_gpu,
        audio_path,
    )
    _WORKFLOW_MODEL_CACHE["key"] = key
    return _WORKFLOW_MODEL_CACHE["model"]


def generate_with_cached_model(
    model_path: str,
    vad_model_path: str | None,
    speaker_model_path: str | None,
    punc_model_path: str | None,
    use_gpu: bool,
    audio_path: str,
) -> Any:
    max_retries = 3
    last_error: Exception | None = None
    for attempt in range(max_retries):
        try:
            model = get_cached_workflow_model(
                model_path,
                vad_model_path,
                speaker_model_path,
                punc_model_path,
                use_gpu,
                audio_path,
            )
            return model.generate(input=audio_path)
        except Exception as exc:
            last_error = exc
            error_msg = str(exc).lower()
            is_oom = any(
                token in error_msg
                for token in ("out of memory", "oom", "cuda out of memory", "allocation")
            )
            if is_oom:
                _WORKFLOW_MODEL_CACHE["key"] = None
                _WORKFLOW_MODEL_CACHE["model"] = None
            if attempt >= max_retries - 1 or not is_oom:
                break
    raise RuntimeError(f"FunASR workflow inference failed: {last_error}") from last_error


def first_result_dict(result: Any) -> Dict[str, Any]:
    if isinstance(result, list) and result:
        first = result[0]
        return first if isinstance(first, dict) else {"text": str(first)}
    if isinstance(result, dict):
        return result
    if result is None:
        return {}
    return {"text": str(result)}


def segment_text(item: Dict[str, Any]) -> str:
    value = item.get("text")
    if value is None:
        value = item.get("sentence")
    return str(value or "").strip()


def segment_speaker_index(item: Dict[str, Any]) -> int:
    value = item.get("spk", item.get("speaker", item.get("speaker_id", 0)))
    try:
        return int(value)
    except (TypeError, ValueError):
        text = str(value).strip()
        digits = "".join(char for char in text if char.isdigit())
        return int(digits) if digits else 0


def segment_time_ms(item: Dict[str, Any], primary: str, fallback: str, default: int) -> int:
    value = item.get(primary, item.get(fallback, default))
    try:
        return max(0, int(float(value)))
    except (TypeError, ValueError):
        return default


def parse_sentence_info(result_dict: Dict[str, Any], duration_seconds: float | None) -> List[TranscriptSegment]:
    sentence_info = result_dict.get("sentence_info")
    segments: List[TranscriptSegment] = []
    if isinstance(sentence_info, list):
        for index, item in enumerate(sentence_info):
            if not isinstance(item, dict):
                continue
            text = segment_text(item)
            if not text:
                continue
            start_ms = segment_time_ms(item, "start", "start_ms", 0)
            end_ms = segment_time_ms(item, "end", "end_ms", start_ms)
            speaker_index = segment_speaker_index(item)
            segments.append(
                TranscriptSegment(
                    id=len(segments),
                    start_ms=start_ms,
                    end_ms=max(start_ms, end_ms),
                    speaker_label=f"Speaker {speaker_index}",
                    speaker_index=speaker_index,
                    text=text,
                )
            )

    if segments:
        return segments

    text = str(result_dict.get("text") or "").strip()
    if not text:
        return []
    end_ms = int((duration_seconds or 0.0) * 1000)
    return [
        TranscriptSegment(
            id=0,
            start_ms=0,
            end_ms=max(0, end_ms),
            speaker_label="Speaker 0",
            speaker_index=0,
            text=text,
        )
    ]


def format_timestamped_transcript_text(segments: Sequence[TranscriptSegment | Dict[str, Any]]) -> str:
    lines: List[str] = []
    for segment in segments:
        if isinstance(segment, TranscriptSegment):
            start_ms = segment.start_ms
            end_ms = segment.end_ms
            speaker_label = segment.speaker_label
            text = segment.text
        else:
            start_ms = int(segment["start_ms"])
            end_ms = int(segment["end_ms"])
            speaker_label = str(segment["speaker_label"])
            text = str(segment["text"])
        text = text.strip()
        if not text:
            continue
        lines.append(f"[{start_ms}ms - {end_ms}ms] {speaker_label}: {text}")
    return "\n".join(lines)


def transcript_segment_to_dict(segment: TranscriptSegment) -> Dict[str, Any]:
    return {
        "id": segment.id,
        "start_ms": segment.start_ms,
        "end_ms": segment.end_ms,
        "speaker_label": segment.speaker_label,
        "speaker_index": segment.speaker_index,
        "text": segment.text,
        "confidence": segment.confidence,
    }


def json_safe(value: Any) -> Any:
    if isinstance(value, dict):
        return {str(key): json_safe(item) for key, item in value.items()}
    if isinstance(value, list):
        return [json_safe(item) for item in value]
    if isinstance(value, tuple):
        return [json_safe(item) for item in value]
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    if hasattr(value, "tolist"):
        try:
            return json_safe(value.tolist())
        except Exception:
            pass
    return str(value)


def transcribe(payload: Dict[str, Any]) -> Dict[str, Any]:
    total_start = time.time()
    audio_path = payload["audio_path"]
    normalized_path = payload["normalized_path"]
    model_path = payload["model_path"]
    ffmpeg_path = payload.get("ffmpeg_path")
    use_gpu = bool(payload.get("use_gpu", True))
    profile = normalize_profile(payload.get("profile"))
    audio_already_normalized = bool(payload.get("audio_already_normalized", False))

    if not Path(audio_path).exists():
        raise FileNotFoundError(f"Audio file not found: {audio_path}")
    if not ffmpeg_path:
        raise RuntimeError("Bundled ffmpeg path was not provided")
    if not ffmpeg_exists(ffmpeg_path):
        raise RuntimeError(f"ffmpeg not found: {ffmpeg_path}")
    prepend_executable_dir_to_path(ffmpeg_path)
    require_local_model_path(model_path, "ASR")

    auxiliary_paths = auxiliary_model_paths(auxiliary_root_for_model(model_path))
    punc_model_path = require_local_model_path(
        str(payload.get("punc_model_path") or auxiliary_paths["punc_model_path"]),
        "Punctuation",
    )
    if profile == DICTATION_PROFILE:
        vad_model_path = None
        speaker_model_path = None
    else:
        vad_model_path = require_local_model_path(
            str(payload.get("vad_model_path") or auxiliary_paths["vad_model_path"]),
            "VAD",
        )
        speaker_model_path = require_local_model_path(
            str(payload.get("speaker_model_path") or auxiliary_paths["speaker_model_path"]),
            "Speaker",
        )

    normalize_elapsed = 0.0
    inference_audio_path = audio_path
    if not audio_already_normalized:
        normalize_start = time.time()
        normalize_audio(ffmpeg_path, audio_path, normalized_path)
        normalize_elapsed = time.time() - normalize_start
        inference_audio_path = normalized_path
    duration_seconds = wav_duration_seconds(inference_audio_path)

    inference_start = time.time()
    if bool(payload.get("reuse_model", False)):
        result = generate_with_cached_model(
            model_path,
            vad_model_path,
            speaker_model_path,
            punc_model_path,
            use_gpu,
            inference_audio_path,
        )
    else:
        result = generate_with_retries(
            model_path,
            vad_model_path,
            speaker_model_path,
            punc_model_path,
            use_gpu,
            inference_audio_path,
        )
    elapsed = time.time() - inference_start
    total_elapsed = time.time() - total_start

    result_dict = first_result_dict(result)
    segments = parse_sentence_info(result_dict, duration_seconds)
    if not segments:
        raise SidecarError("EMPTY_TRANSCRIPT", "ASR returned empty transcript")

    text = format_timestamped_transcript_text(segments)
    plain_text = str(result_dict.get("text") or "").strip() or "\n".join(
        segment.text for segment in segments
    )
    speaker_count = len({segment.speaker_index for segment in segments})

    return {
        "profile": profile,
        "text": text,
        "plain_text": plain_text,
        "segments": [transcript_segment_to_dict(segment) for segment in segments],
        "duration_seconds": duration_seconds or elapsed,
        "rtf": elapsed / duration_seconds if duration_seconds and duration_seconds > 0 else None,
        "language": None,
        "confidence": None,
        "segment_count": len(segments),
        "speaker_count": speaker_count,
        "engine": {
            "name": ENGINE_NAME,
            "profile": profile,
            "asr_model": DEFAULT_ASR_MODEL,
            "model_revision": DEFAULT_FUNASR_MODEL_REVISION,
            "model_path": model_path,
            "vad_model": DEFAULT_VAD_MODEL_NAME,
            "vad_model_revision": DEFAULT_FUNASR_MODEL_REVISION if vad_model_path else None,
            "vad_model_path": vad_model_path,
            "speaker_model": DEFAULT_SPEAKER_MODEL_NAME,
            "speaker_model_revision": None,
            "speaker_model_path": speaker_model_path,
            "punc_model": DEFAULT_PUNC_MODEL_NAME,
            "punc_model_revision": DEFAULT_FUNASR_MODEL_REVISION if punc_model_path else None,
            "punc_model_path": punc_model_path,
            "device": get_inference_device(use_gpu),
        },
        "timing": {
            "normalize_audio_ms": int(normalize_elapsed * 1000),
            "asr_infer_ms": int(elapsed * 1000),
            "total_asr_ms": int(total_elapsed * 1000),
        },
        "normalized_audio_path": inference_audio_path,
        "funasr_result": json_safe(result),
    }


def warmup(payload: Dict[str, Any]) -> Dict[str, Any]:
    total_start = time.time()
    audio_path = payload["audio_path"]
    model_path = payload["model_path"]
    use_gpu = bool(payload.get("use_gpu", True))
    profile = normalize_profile(payload.get("profile"))

    if not Path(audio_path).exists():
        raise FileNotFoundError(f"Warmup audio file not found: {audio_path}")
    require_local_model_path(model_path, "ASR")

    auxiliary_paths = auxiliary_model_paths(auxiliary_root_for_model(model_path))
    punc_model_path = require_local_model_path(
        str(payload.get("punc_model_path") or auxiliary_paths["punc_model_path"]),
        "Punctuation",
    )
    if profile == DICTATION_PROFILE:
        vad_model_path = None
        speaker_model_path = None
    else:
        vad_model_path = require_local_model_path(
            str(payload.get("vad_model_path") or auxiliary_paths["vad_model_path"]),
            "VAD",
        )
        speaker_model_path = require_local_model_path(
            str(payload.get("speaker_model_path") or auxiliary_paths["speaker_model_path"]),
            "Speaker",
        )

    model_start = time.time()
    model = get_cached_workflow_model(
        model_path,
        vad_model_path,
        speaker_model_path,
        punc_model_path,
        use_gpu,
        audio_path,
    )
    model_elapsed = time.time() - model_start

    infer_start = time.time()
    result = model.generate(input=audio_path)
    infer_elapsed = time.time() - infer_start
    total_elapsed = time.time() - total_start

    return {
        "profile": profile,
        "warmup_audio_path": audio_path,
        "engine": {
            "name": ENGINE_NAME,
            "profile": profile,
            "asr_model": DEFAULT_ASR_MODEL,
            "model_revision": DEFAULT_FUNASR_MODEL_REVISION,
            "model_path": model_path,
            "vad_model": DEFAULT_VAD_MODEL_NAME,
            "vad_model_revision": DEFAULT_FUNASR_MODEL_REVISION if vad_model_path else None,
            "vad_model_path": vad_model_path,
            "speaker_model": DEFAULT_SPEAKER_MODEL_NAME,
            "speaker_model_revision": None,
            "speaker_model_path": speaker_model_path,
            "punc_model": DEFAULT_PUNC_MODEL_NAME,
            "punc_model_revision": DEFAULT_FUNASR_MODEL_REVISION if punc_model_path else None,
            "punc_model_path": punc_model_path,
            "device": get_inference_device(use_gpu),
        },
        "timing": {
            "model_warmup_ms": int(model_elapsed * 1000),
            "warmup_infer_ms": int(infer_elapsed * 1000),
            "total_warmup_ms": int(total_elapsed * 1000),
        },
        "funasr_result": json_safe(result),
    }


def handle_request(request: Dict[str, Any]) -> int:
    try:
        req_id = str(request.get("id", "unknown"))
        req_type = request.get("type")
        payload = request.get("payload", {})

        if req_type == "shutdown":
            emit(response_ok(req_id, {"shutdown": True}))
            return 2

        if req_type == "prepare_model":
            repo = payload.get("repo") or DEFAULT_ASR_MODEL
            target_dir = payload["target_dir"]
            source = payload.get("source", "modelscope")
            emit(response_ok(req_id, prepare_pipeline_models(repo, target_dir, source)))
            return 0

        if req_type == "prepare_auxiliary_models":
            target_dir = payload["target_dir"]
            source = payload.get("source", "modelscope")
            emit(response_ok(req_id, {"source": source, **prepare_auxiliary_models(target_dir, source)}))
            return 0

        if req_type == "prepare_punctuation_model":
            target_dir = payload["target_dir"]
            source = payload.get("source", "modelscope")
            emit(response_ok(req_id, {"source": source, **prepare_punctuation_model(target_dir, source)}))
            return 0

        if req_type == "prepare_dictation_models":
            repo = payload.get("repo") or DEFAULT_ASR_MODEL
            target_dir = payload["target_dir"]
            source = payload.get("source", "modelscope")
            emit(response_ok(req_id, prepare_dictation_models(repo, target_dir, source)))
            return 0

        if req_type == "estimate_model_download_size":
            repo = payload.get("repo") or DEFAULT_ASR_MODEL
            source = payload.get("source", "modelscope")
            profile = payload.get("profile", DEFAULT_PROFILE)
            emit(response_ok(req_id, estimate_model_download_size(repo, source, profile)))
            return 0

        if req_type == "transcribe":
            result = transcribe(payload)
            emit(response_ok(req_id, result))
            return 0

        if req_type == "warmup":
            result = warmup(payload)
            emit(response_ok(req_id, result))
            return 0

        emit(response_error(req_id, "UNKNOWN_REQUEST", f"Unsupported request type: {req_type}"))
        return 1
    except SidecarError as exc:
        req_id = "unknown"
        try:
            if "request" in locals():
                req_id = str(request.get("id", "unknown"))
        except Exception:
            pass
        emit(response_error(req_id, exc.code, exc.message))
        return 1
    except Exception as exc:  # pragma: no cover - surfaced to caller
        req_id = "unknown"
        try:
            if "request" in locals():
                req_id = str(request.get("id", "unknown"))
        except Exception:
            pass
        emit(response_error(req_id, type(exc).__name__.upper(), str(exc)))
        return 1


def run_once() -> int:
    return handle_request(read_request())


def run_server() -> int:
    while True:
        raw = sys.stdin.readline()
        if not raw:
            return 0
        raw = raw.strip()
        if not raw:
            continue
        try:
            request = json.loads(raw)
        except Exception as exc:
            emit(response_error("unknown", "INVALID_JSON", str(exc)))
            continue
        result = handle_request(request)
        if result == 2:
            return 0


def main() -> int:
    if "--server" in sys.argv:
        return run_server()
    return run_once()


if __name__ == "__main__":
    raise SystemExit(main())
