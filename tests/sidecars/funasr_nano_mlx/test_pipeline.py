from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
import wave
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
MODULE_DIR = REPO_ROOT / "sidecars" / "funasr_nano_mlx"
sys.path.insert(0, str(MODULE_DIR))

from main import (
    SidecarError,
    TranscriptSegment,
    format_timestamped_transcript_text,
    get_inference_device,
    parse_sentence_info,
    prepend_executable_dir_to_path,
    prepare_auxiliary_models,
    prepare_dictation_models,
    prepare_pipeline_models,
    prepare_punctuation_model,
    transcribe,
)


LINE_PATTERN = re.compile(r"^\[(\d+)ms - (\d+)ms\] Speaker \d+: .+$")


class FunAsrWorkflowPipelineTests(unittest.TestCase):
    def resolve_local_ffmpeg(self) -> str:
        bundled_ffmpeg = REPO_ROOT / "runtime" / "asr" / "bin" / "ffmpeg"
        if bundled_ffmpeg.exists():
            return str(bundled_ffmpeg)
        system_ffmpeg = shutil.which("ffmpeg")
        if system_ffmpeg:
            return system_ffmpeg
        self.skipTest("ffmpeg is required for the real workflow sidecar aspect test")

    def write_silent_wav(self, path: Path, sample_rate: int = 8000, duration_seconds: float = 0.25) -> None:
        frame_count = int(sample_rate * duration_seconds)
        with wave.open(str(path), "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(sample_rate)
            wav.writeframes(b"\x00\x00" * frame_count)

    def write_fun_asr_aspect(self, aspect_dir: Path) -> Path:
        aspect_path = aspect_dir / "funasr.py"
        aspect_path.write_text(
            """
import json
import os
from pathlib import Path


def _log(payload):
    log_path = os.environ.get("VOICE_VIBE_FUNASR_ASPECT_LOG")
    if not log_path:
        return
    with open(log_path, "a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload, ensure_ascii=False) + "\\n")


class AutoModel:
    def __init__(self, **kwargs):
        self.kwargs = kwargs
        _log({
            "event": "init",
            "model": kwargs.get("model"),
            "vad_model": kwargs.get("vad_model"),
            "spk_model": kwargs.get("spk_model"),
            "punc_model": kwargs.get("punc_model"),
            "model_revision": kwargs.get("model_revision"),
            "vad_model_revision": kwargs.get("vad_model_revision"),
            "spk_model_revision": kwargs.get("spk_model_revision"),
            "punc_model_revision": kwargs.get("punc_model_revision"),
            "device": kwargs.get("device"),
            "batch_size": kwargs.get("batch_size"),
            "ncpu": kwargs.get("ncpu"),
        })

    def generate(self, input):
        path = Path(input)
        _log({
            "event": "generate",
            "input": str(path),
            "input_exists": path.exists(),
            "input_size": path.stat().st_size if path.exists() else None,
        })
        return [{
            "text": "大家好。收到。",
            "sentence_info": [
                {"start": 0, "end": 320, "spk": 0, "text": "大家好。"},
                {"start": 320, "end": 760, "spk": 1, "text": "收到。"},
            ],
        }]
""".lstrip(),
            encoding="utf-8",
        )
        return aspect_path

    def inference_device_with_fake_torch(
        self,
        *,
        cuda_available: bool,
        mps_available: bool,
        requested: str = "auto",
        use_gpu: bool = True,
    ) -> str:
        class FakeCuda:
            @staticmethod
            def is_available() -> bool:
                return cuda_available

        class FakeMps:
            @staticmethod
            def is_available() -> bool:
                return mps_available

        class FakeBackends:
            mps = FakeMps()

        class FakeTorch:
            cuda = FakeCuda()
            backends = FakeBackends()

        original_torch = sys.modules.get("torch")
        original_requested_device = os.environ.get("VOICE_VIBE_DEVICE")
        original_mps_fallback = os.environ.get("PYTORCH_ENABLE_MPS_FALLBACK")
        try:
            sys.modules["torch"] = FakeTorch()
            os.environ["VOICE_VIBE_DEVICE"] = requested
            os.environ.pop("PYTORCH_ENABLE_MPS_FALLBACK", None)
            return get_inference_device(use_gpu)
        finally:
            if original_torch is None:
                sys.modules.pop("torch", None)
            else:
                sys.modules["torch"] = original_torch
            if original_requested_device is None:
                os.environ.pop("VOICE_VIBE_DEVICE", None)
            else:
                os.environ["VOICE_VIBE_DEVICE"] = original_requested_device
            if original_mps_fallback is None:
                os.environ.pop("PYTORCH_ENABLE_MPS_FALLBACK", None)
            else:
                os.environ["PYTORCH_ENABLE_MPS_FALLBACK"] = original_mps_fallback

    def test_inference_device_auto_prefers_cuda_then_mps_then_cpu(self) -> None:
        self.assertEqual(
            self.inference_device_with_fake_torch(cuda_available=True, mps_available=True),
            "cuda",
        )
        self.assertEqual(
            self.inference_device_with_fake_torch(cuda_available=False, mps_available=True),
            "mps",
        )
        self.assertEqual(
            self.inference_device_with_fake_torch(cuda_available=False, mps_available=False),
            "cpu",
        )

    def test_inference_device_honors_cpu_and_gpu_overrides(self) -> None:
        self.assertEqual(
            self.inference_device_with_fake_torch(
                cuda_available=True,
                mps_available=True,
                requested="auto",
                use_gpu=False,
            ),
            "cpu",
        )
        self.assertEqual(
            self.inference_device_with_fake_torch(
                cuda_available=False,
                mps_available=True,
                requested="gpu",
            ),
            "mps",
        )
        self.assertEqual(
            self.inference_device_with_fake_torch(
                cuda_available=False,
                mps_available=False,
                requested="mps",
            ),
            "cpu",
        )

    def test_format_timestamped_transcript_text_matches_parser_regex(self) -> None:
        text = format_timestamped_transcript_text(
            [
                TranscriptSegment(0, 50, 410, "Speaker 0", 0, "对的，"),
                TranscriptSegment(1, 410, 2910, "Speaker 0", 0, "因为这个也是我们擅长的，"),
                TranscriptSegment(2, 36230, 41030, "Speaker 1", 1, "所以就是因为深圳深源拓科技，"),
            ]
        )

        lines = text.splitlines()
        self.assertEqual(lines[0], "[50ms - 410ms] Speaker 0: 对的，")
        self.assertTrue(all(LINE_PATTERN.match(line) for line in lines))

    def test_parse_sentence_info_uses_workflow_speaker_and_timestamps(self) -> None:
        segments = parse_sentence_info(
            {
                "sentence_info": [
                    {"start": 10, "end": 550, "spk": 2, "text": "大家好。"},
                    {"start": 600, "end": 980, "spk": "Speaker 3", "sentence": "继续。"},
                ],
                "text": "大家好。继续。",
            },
            1.0,
        )

        self.assertEqual(len(segments), 2)
        self.assertEqual(segments[0].speaker_label, "Speaker 2")
        self.assertEqual(segments[1].speaker_index, 3)
        self.assertEqual(segments[1].text, "继续。")

    def test_sidecar_transcribe_request_exercises_real_workflow_with_fun_asr_aspect(self) -> None:
        sidecar_dir = MODULE_DIR
        ffmpeg_path = self.resolve_local_ffmpeg()

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            aspect_dir = tmp / "aspect"
            aspect_dir.mkdir()
            self.write_fun_asr_aspect(aspect_dir)

            audio_path = tmp / "input.wav"
            normalized_path = tmp / "normalized.wav"
            model_path = tmp / "model"
            vad_model_path = model_path / ".voice_vibe_aux" / "fsmn-vad"
            speaker_model_path = model_path / ".voice_vibe_aux" / "campplus"
            punc_model_path = model_path / ".voice_vibe_aux" / "ct-punc-c"
            for path in (model_path, vad_model_path, speaker_model_path, punc_model_path):
                path.mkdir(parents=True, exist_ok=True)
            self.write_silent_wav(audio_path)

            request = {
                "id": "aspect-transcribe",
                "type": "transcribe",
                "payload": {
                    "audio_path": str(audio_path),
                    "normalized_path": str(normalized_path),
                    "model_path": str(model_path),
                    "ffmpeg_path": ffmpeg_path,
                    "use_gpu": True,
                    "vad_model_path": str(vad_model_path),
                    "speaker_model_path": str(speaker_model_path),
                    "punc_model_path": str(punc_model_path),
                },
            }

            aspect_log = tmp / "funasr-aspect.jsonl"
            env = os.environ.copy()
            existing_pythonpath = env.get("PYTHONPATH")
            env["PYTHONPATH"] = os.pathsep.join(
                [str(aspect_dir), str(sidecar_dir)]
                + ([existing_pythonpath] if existing_pythonpath else [])
            )
            env["VOICE_VIBE_DEVICE"] = "auto"
            env["VOICE_VIBE_FUNASR_ASPECT_LOG"] = str(aspect_log)

            completed = subprocess.run(
                [sys.executable, str(sidecar_dir / "main.py")],
                input=json.dumps(request, ensure_ascii=False) + "\n",
                capture_output=True,
                check=False,
                env=env,
                text=True,
                timeout=30,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)

            response = json.loads(completed.stdout.strip().splitlines()[-1])
            self.assertTrue(response["ok"], response)
            result = response["result"]
            self.assertEqual(
                result["text"],
                "[0ms - 320ms] Speaker 0: 大家好。\n[320ms - 760ms] Speaker 1: 收到。",
            )
            self.assertEqual(result["plain_text"], "大家好。收到。")
            self.assertEqual(result["segment_count"], 2)
            self.assertEqual(result["speaker_count"], 2)
            self.assertEqual(result["normalized_audio_path"], str(normalized_path))
            self.assertTrue(normalized_path.exists())

            with wave.open(str(normalized_path), "rb") as wav:
                self.assertEqual(wav.getframerate(), 16000)
                self.assertEqual(wav.getnchannels(), 1)

            aspect_events = [
                json.loads(line)
                for line in aspect_log.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            init_event = next(event for event in aspect_events if event["event"] == "init")
            generate_event = next(event for event in aspect_events if event["event"] == "generate")
            self.assertEqual(init_event["model"], str(model_path))
            self.assertEqual(init_event["vad_model"], str(vad_model_path))
            self.assertEqual(init_event["spk_model"], str(speaker_model_path))
            self.assertEqual(init_event["punc_model"], str(punc_model_path))
            self.assertEqual(init_event["model_revision"], "v2.0.4")
            self.assertEqual(init_event["vad_model_revision"], "v2.0.4")
            self.assertIsNone(init_event["spk_model_revision"])
            self.assertEqual(init_event["punc_model_revision"], "v2.0.4")
            self.assertEqual(init_event["device"], result["engine"]["device"])
            self.assertIn(init_event["device"], {"cpu", "cuda", "mps"})
            self.assertEqual(generate_event["input"], str(normalized_path))
            self.assertTrue(generate_event["input_exists"])

    def test_dictation_profile_skips_vad_speaker_and_audio_normalization(self) -> None:
        sidecar_dir = MODULE_DIR
        ffmpeg_path = self.resolve_local_ffmpeg()

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            aspect_dir = tmp / "aspect"
            aspect_dir.mkdir()
            self.write_fun_asr_aspect(aspect_dir)

            audio_path = tmp / "input.wav"
            normalized_path = tmp / "normalized.wav"
            model_path = tmp / "model"
            punc_model_path = model_path / ".voice_vibe_aux" / "ct-punc-c"
            for path in (model_path, punc_model_path):
                path.mkdir(parents=True, exist_ok=True)
            self.write_silent_wav(audio_path, sample_rate=16000)

            request = {
                "id": "dictation-transcribe",
                "type": "transcribe",
                "payload": {
                    "audio_path": str(audio_path),
                    "normalized_path": str(normalized_path),
                    "model_path": str(model_path),
                    "ffmpeg_path": ffmpeg_path,
                    "use_gpu": True,
                    "punc_model_path": str(punc_model_path),
                    "profile": "dictation",
                    "audio_already_normalized": True,
                },
            }

            aspect_log = tmp / "funasr-aspect.jsonl"
            env = os.environ.copy()
            existing_pythonpath = env.get("PYTHONPATH")
            env["PYTHONPATH"] = os.pathsep.join(
                [str(aspect_dir), str(sidecar_dir)]
                + ([existing_pythonpath] if existing_pythonpath else [])
            )
            env["VOICE_VIBE_DEVICE"] = "auto"
            env["VOICE_VIBE_FUNASR_ASPECT_LOG"] = str(aspect_log)

            completed = subprocess.run(
                [sys.executable, str(sidecar_dir / "main.py")],
                input=json.dumps(request, ensure_ascii=False) + "\n",
                capture_output=True,
                check=False,
                env=env,
                text=True,
                timeout=30,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)

            response = json.loads(completed.stdout.strip().splitlines()[-1])
            self.assertTrue(response["ok"], response)
            result = response["result"]
            self.assertEqual(result["profile"], "dictation")
            self.assertEqual(result["engine"]["profile"], "dictation")
            self.assertEqual(result["normalized_audio_path"], str(audio_path))
            self.assertFalse(normalized_path.exists())
            self.assertIn("timing", result)
            self.assertEqual(result["timing"]["normalize_audio_ms"], 0)
            self.assertGreaterEqual(result["timing"]["asr_infer_ms"], 0)

            aspect_events = [
                json.loads(line)
                for line in aspect_log.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            init_event = next(event for event in aspect_events if event["event"] == "init")
            generate_event = next(event for event in aspect_events if event["event"] == "generate")
            self.assertEqual(init_event["model"], str(model_path))
            self.assertIsNone(init_event["vad_model"])
            self.assertIsNone(init_event["spk_model"])
            self.assertEqual(init_event["punc_model"], str(punc_model_path))
            self.assertEqual(init_event["model_revision"], "v2.0.4")
            self.assertIsNone(init_event["vad_model_revision"])
            self.assertIsNone(init_event["spk_model_revision"])
            self.assertEqual(init_event["punc_model_revision"], "v2.0.4")
            self.assertEqual(generate_event["input"], str(audio_path))
            self.assertTrue(generate_event["input_exists"])

    def test_dictation_warmup_uses_small_audio_to_run_generate_once(self) -> None:
        sidecar_dir = MODULE_DIR

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            aspect_dir = tmp / "aspect"
            aspect_dir.mkdir()
            self.write_fun_asr_aspect(aspect_dir)

            model_path = tmp / "model"
            punc_model_path = model_path / ".voice_vibe_aux" / "ct-punc-c"
            for path in (model_path, punc_model_path):
                path.mkdir(parents=True, exist_ok=True)
            warmup_audio_path = tmp / "warmup.wav"
            self.write_silent_wav(warmup_audio_path, sample_rate=16000, duration_seconds=0.5)

            request = {
                "id": "dictation-warmup",
                "type": "warmup",
                "payload": {
                    "audio_path": str(warmup_audio_path),
                    "model_path": str(model_path),
                    "use_gpu": True,
                    "punc_model_path": str(punc_model_path),
                    "profile": "dictation",
                },
            }

            aspect_log = tmp / "funasr-aspect.jsonl"
            env = os.environ.copy()
            existing_pythonpath = env.get("PYTHONPATH")
            env["PYTHONPATH"] = os.pathsep.join(
                [str(aspect_dir), str(sidecar_dir)]
                + ([existing_pythonpath] if existing_pythonpath else [])
            )
            env["VOICE_VIBE_DEVICE"] = "auto"
            env["VOICE_VIBE_FUNASR_ASPECT_LOG"] = str(aspect_log)

            completed = subprocess.run(
                [sys.executable, str(sidecar_dir / "main.py")],
                input=json.dumps(request, ensure_ascii=False) + "\n",
                capture_output=True,
                check=False,
                env=env,
                text=True,
                timeout=30,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)

            response = json.loads(completed.stdout.strip().splitlines()[-1])
            self.assertTrue(response["ok"], response)
            result = response["result"]
            self.assertEqual(result["profile"], "dictation")
            self.assertEqual(result["warmup_audio_path"], str(warmup_audio_path))
            self.assertEqual(result["engine"]["profile"], "dictation")
            self.assertIsNone(result["engine"]["vad_model_path"])
            self.assertIsNone(result["engine"]["speaker_model_path"])
            self.assertEqual(result["engine"]["punc_model_path"], str(punc_model_path))
            self.assertIn("timing", result)
            self.assertGreaterEqual(result["timing"]["model_warmup_ms"], 0)
            self.assertGreaterEqual(result["timing"]["warmup_infer_ms"], 0)

            aspect_events = [
                json.loads(line)
                for line in aspect_log.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            self.assertEqual([event["event"] for event in aspect_events], ["init", "generate"])
            generate_event = next(event for event in aspect_events if event["event"] == "generate")
            self.assertEqual(generate_event["input"], str(warmup_audio_path))

    def test_prepare_pipeline_models_downloads_workflow_model_set(self) -> None:
        import main

        calls: list[tuple[str, str, str | None]] = []
        original_download_modelscope = main.download_modelscope_model

        def fake_download(repo: str, target_dir: str, revision: str | None = None) -> str:
            calls.append((repo, target_dir, revision))
            Path(target_dir).mkdir(parents=True, exist_ok=True)
            return target_dir

        try:
            main.download_modelscope_model = fake_download
            with tempfile.TemporaryDirectory() as tmpdir:
                result = prepare_pipeline_models("paraformer-zh", str(Path(tmpdir) / "asr"), "modelscope")
        finally:
            main.download_modelscope_model = original_download_modelscope

        self.assertEqual(
            [repo for repo, _target, _revision in calls],
            [
                "iic/speech_seaco_paraformer_large_asr_nat-zh-cn-16k-common-vocab8404-pytorch",
                "iic/speech_fsmn_vad_zh-cn-16k-common-pytorch",
                "iic/speech_campplus_sv_zh-cn_16k-common",
                "iic/punc_ct-transformer_zh-cn-common-vocab272727-pytorch",
            ],
        )
        self.assertEqual(
            [revision for _repo, _target, revision in calls],
            ["v2.0.4", "v2.0.4", None, "v2.0.4"],
        )
        self.assertEqual(result["repo"], "paraformer-zh")
        self.assertTrue(result["vad_model_path"].endswith(".voice_vibe_aux/fsmn-vad"))
        self.assertTrue(result["speaker_model_path"].endswith(".voice_vibe_aux/campplus"))
        self.assertTrue(result["punc_model_path"].endswith(".voice_vibe_aux/ct-punc-c"))

    def test_prepare_dictation_models_downloads_asr_and_punctuation_only(self) -> None:
        import main

        calls: list[tuple[str, str, str | None]] = []
        original_download_modelscope = main.download_modelscope_model

        def fake_download(repo: str, target_dir: str, revision: str | None = None) -> str:
            calls.append((repo, target_dir, revision))
            Path(target_dir).mkdir(parents=True, exist_ok=True)
            return target_dir

        try:
            main.download_modelscope_model = fake_download
            with tempfile.TemporaryDirectory() as tmpdir:
                result = prepare_dictation_models("paraformer-zh", str(Path(tmpdir) / "asr"), "modelscope")
        finally:
            main.download_modelscope_model = original_download_modelscope

        self.assertEqual(
            [repo for repo, _target, _revision in calls],
            [
                "iic/speech_seaco_paraformer_large_asr_nat-zh-cn-16k-common-vocab8404-pytorch",
                "iic/punc_ct-transformer_zh-cn-common-vocab272727-pytorch",
            ],
        )
        self.assertEqual(
            [revision for _repo, _target, revision in calls],
            ["v2.0.4", "v2.0.4"],
        )
        self.assertEqual(result["profile"], "dictation")
        self.assertEqual(result["repo"], "paraformer-zh")
        self.assertTrue(result["punc_model_path"].endswith(".voice_vibe_aux/ct-punc-c"))
        self.assertNotIn("vad_model_path", result)
        self.assertNotIn("speaker_model_path", result)

    def test_prepare_dictation_models_uses_huggingface_default_revision(self) -> None:
        import main

        calls: list[tuple[str, str, str | None]] = []
        original_download_huggingface = main.download_huggingface_model

        def fake_download(repo: str, target_dir: str, revision: str | None = None) -> str:
            calls.append((repo, target_dir, revision))
            Path(target_dir).mkdir(parents=True, exist_ok=True)
            return target_dir

        try:
            main.download_huggingface_model = fake_download
            with tempfile.TemporaryDirectory() as tmpdir:
                result = prepare_dictation_models("paraformer-zh", str(Path(tmpdir) / "asr"), "huggingface")
        finally:
            main.download_huggingface_model = original_download_huggingface

        self.assertEqual(
            [repo for repo, _target, _revision in calls],
            [
                "funasr/paraformer-zh",
                "funasr/ct-punc",
            ],
        )
        self.assertEqual([revision for _repo, _target, revision in calls], [None, None])
        self.assertIsNone(result["model_revision"])
        self.assertIsNone(result["auxiliary_models"]["punc"]["revision"])

    def test_estimate_dictation_download_size_uses_huggingface_default_revisions(self) -> None:
        import main

        calls: list[tuple[str, str, str | None]] = []
        original_estimate = main.estimate_repo_download_size

        def fake_estimate(repo: str, source: str, revision: str | None = None) -> int:
            calls.append((repo, source, revision))
            return {
                "funasr/paraformer-zh": 889_377_240,
                "funasr/ct-punc": 1_133_956_825,
            }[repo]

        try:
            main.estimate_repo_download_size = fake_estimate
            result = main.estimate_model_download_size("paraformer-zh", "huggingface", "dictation")
        finally:
            main.estimate_repo_download_size = original_estimate

        self.assertEqual(
            calls,
            [
                ("funasr/paraformer-zh", "huggingface", None),
                ("funasr/ct-punc", "huggingface", None),
            ],
        )
        self.assertEqual(result["total_bytes"], 2_023_334_065)

    def test_estimate_pipeline_download_size_uses_modelscope_revisions(self) -> None:
        import main

        calls: list[tuple[str, str, str | None]] = []
        original_estimate = main.estimate_repo_download_size

        def fake_estimate(repo: str, source: str, revision: str | None = None) -> int:
            calls.append((repo, source, revision))
            return {
                "iic/speech_seaco_paraformer_large_asr_nat-zh-cn-16k-common-vocab8404-pytorch": 998_683_485,
                "iic/speech_fsmn_vad_zh-cn-16k-common-pytorch": 4_029_275,
                "iic/speech_campplus_sv_zh-cn_16k-common": 28_855_824,
                "iic/punc_ct-transformer_zh-cn-common-vocab272727-pytorch": 296_361_612,
            }[repo]

        try:
            main.estimate_repo_download_size = fake_estimate
            result = main.estimate_model_download_size("paraformer-zh", "modelscope", "pipeline")
        finally:
            main.estimate_repo_download_size = original_estimate

        self.assertEqual(
            calls,
            [
                (
                    "iic/speech_seaco_paraformer_large_asr_nat-zh-cn-16k-common-vocab8404-pytorch",
                    "modelscope",
                    "v2.0.4",
                ),
                ("iic/speech_fsmn_vad_zh-cn-16k-common-pytorch", "modelscope", "v2.0.4"),
                ("iic/speech_campplus_sv_zh-cn_16k-common", "modelscope", None),
                (
                    "iic/punc_ct-transformer_zh-cn-common-vocab272727-pytorch",
                    "modelscope",
                    "v2.0.4",
                ),
            ],
        )
        self.assertEqual(result["total_bytes"], 1_327_930_196)

    def test_prepare_punctuation_model_downloads_only_punctuation(self) -> None:
        import main

        calls: list[tuple[str, str, str | None]] = []
        original_download_modelscope = main.download_modelscope_model

        def fake_download(repo: str, target_dir: str, revision: str | None = None) -> str:
            calls.append((repo, target_dir, revision))
            Path(target_dir).mkdir(parents=True, exist_ok=True)
            return target_dir

        try:
            main.download_modelscope_model = fake_download
            with tempfile.TemporaryDirectory() as tmpdir:
                result = prepare_punctuation_model(tmpdir, "modelscope")
        finally:
            main.download_modelscope_model = original_download_modelscope

        self.assertEqual(
            [repo for repo, _target, _revision in calls],
            ["iic/punc_ct-transformer_zh-cn-common-vocab272727-pytorch"],
        )
        self.assertEqual([revision for _repo, _target, revision in calls], ["v2.0.4"])
        self.assertIn("punc", result["auxiliary_models"])
        self.assertTrue(result["punc_model_path"].endswith("ct-punc-c"))
        self.assertNotIn("vad", result["auxiliary_models"])
        self.assertNotIn("speaker", result["auxiliary_models"])

    def test_prepare_auxiliary_models_downloads_punctuation_model(self) -> None:
        import main

        calls: list[tuple[str, str, str | None]] = []
        original_download_modelscope = main.download_modelscope_model

        def fake_download(repo: str, target_dir: str, revision: str | None = None) -> str:
            calls.append((repo, target_dir, revision))
            Path(target_dir).mkdir(parents=True, exist_ok=True)
            return target_dir

        try:
            main.download_modelscope_model = fake_download
            with tempfile.TemporaryDirectory() as tmpdir:
                result = prepare_auxiliary_models(tmpdir, "modelscope")
        finally:
            main.download_modelscope_model = original_download_modelscope

        self.assertEqual(
            [repo for repo, _target, _revision in calls],
            [
                "iic/speech_fsmn_vad_zh-cn-16k-common-pytorch",
                "iic/speech_campplus_sv_zh-cn_16k-common",
                "iic/punc_ct-transformer_zh-cn-common-vocab272727-pytorch",
            ],
        )
        self.assertEqual(
            [revision for _repo, _target, revision in calls],
            ["v2.0.4", None, "v2.0.4"],
        )
        self.assertIn("punc", result["auxiliary_models"])
        self.assertTrue(result["punc_model_path"].endswith("ct-punc-c"))

    def test_prepend_executable_dir_to_path_exposes_bundled_ffmpeg(self) -> None:
        original_path = os.environ.get("PATH")
        try:
            with tempfile.TemporaryDirectory() as tmpdir:
                bin_dir = Path(tmpdir) / "bin"
                bin_dir.mkdir()
                os.environ["PATH"] = "/usr/bin"

                prepend_executable_dir_to_path(str(bin_dir / "ffmpeg"))

                self.assertEqual(os.environ["PATH"].split(os.pathsep)[0], str(bin_dir))
        finally:
            if original_path is None:
                os.environ.pop("PATH", None)
            else:
                os.environ["PATH"] = original_path

    def test_empty_workflow_result_uses_explicit_error_code(self) -> None:
        import main

        original_normalize = main.normalize_audio
        original_duration = main.wav_duration_seconds
        original_ffmpeg = main.ffmpeg_exists
        original_generate = main.generate_with_retries
        try:
            main.normalize_audio = lambda *_args, **_kwargs: None
            main.wav_duration_seconds = lambda _path: 1.0
            main.ffmpeg_exists = lambda _path: True
            main.generate_with_retries = lambda *_args, **_kwargs: [{"text": "", "sentence_info": []}]

            with tempfile.TemporaryDirectory() as tmpdir:
                tmp = Path(tmpdir)
                audio_path = tmp / "input.wav"
                model_path = tmp / "model"
                audio_path.write_bytes(b"")
                model_path.mkdir()
                (model_path / ".voice_vibe_aux" / "fsmn-vad").mkdir(parents=True)
                (model_path / ".voice_vibe_aux" / "campplus").mkdir(parents=True)
                (model_path / ".voice_vibe_aux" / "ct-punc-c").mkdir(parents=True)

                with self.assertRaises(SidecarError) as raised:
                    transcribe(
                        {
                            "audio_path": str(audio_path),
                            "normalized_path": str(tmp / "normalized.wav"),
                            "model_path": str(model_path),
                            "ffmpeg_path": str(tmp / "ffmpeg"),
                        }
                    )
                self.assertEqual(raised.exception.code, "EMPTY_TRANSCRIPT")
        finally:
            main.normalize_audio = original_normalize
            main.wav_duration_seconds = original_duration
            main.ffmpeg_exists = original_ffmpeg
            main.generate_with_retries = original_generate


if __name__ == "__main__":
    unittest.main()
