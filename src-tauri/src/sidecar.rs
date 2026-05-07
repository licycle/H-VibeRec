use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Instant;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::types::AppSettings;

#[derive(Debug, Clone)]
pub struct AsrRuntime {
    pub root: PathBuf,
    pub python_path: PathBuf,
    pub ffmpeg_path: PathBuf,
    pub script_path: PathBuf,
}

pub fn resolve_asr_runtime(app: &AppHandle) -> Result<AsrRuntime, String> {
    let (runtime_root, script_path) = if cfg!(debug_assertions) {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        (
            repo_root.join("runtime").join("asr"),
            repo_root
                .join("sidecars")
                .join("funasr_nano_mlx")
                .join("main.py"),
        )
    } else {
        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|e| format!("Cannot resolve bundled resource directory: {e}"))?;
        (
            resource_dir.join("runtime").join("asr"),
            resource_dir
                .join("sidecars")
                .join("funasr_nano_mlx")
                .join("main.py"),
        )
    };

    let runtime = AsrRuntime {
        python_path: runtime_root.join(bundled_python_relative_path()),
        ffmpeg_path: runtime_root.join(bundled_ffmpeg_relative_path()),
        root: runtime_root,
        script_path,
    };
    validate_asr_runtime(&runtime)?;
    Ok(runtime)
}

pub fn resolve_assistant_sidecar_script(app: &AppHandle) -> Result<PathBuf, String> {
    let script_path = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("sidecars")
            .join("local_notes_agent")
            .join("main.py")
    } else {
        app.path()
            .resource_dir()
            .map_err(|e| format!("Cannot resolve bundled resource directory: {e}"))?
            .join("sidecars")
            .join("local_notes_agent")
            .join("main.py")
    };

    if !script_path.exists() {
        return Err(format!(
            "Local notes assistant sidecar missing: {}",
            script_path.display()
        ));
    }
    Ok(script_path)
}

pub async fn run_sidecar(
    app: &AppHandle,
    request: Value,
    settings: Option<&AppSettings>,
) -> Result<Value, String> {
    run_sidecar_with_cancel(app, request, settings, None).await
}

pub async fn run_sidecar_with_cancel(
    app: &AppHandle,
    request: Value,
    settings: Option<&AppSettings>,
    cancel: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<Value, String> {
    let runtime = resolve_asr_runtime(app)?;
    let script_path = runtime.script_path.clone();
    run_python_sidecar_with_cancel(runtime, script_path, request, settings, cancel, "ASR").await
}

pub async fn run_assistant_sidecar(
    app: &AppHandle,
    request: Value,
    settings: &AppSettings,
) -> Result<Value, String> {
    let runtime = resolve_asr_runtime(app)?;
    let script_path = resolve_assistant_sidecar_script(app)?;
    run_python_sidecar_with_cancel(
        runtime,
        script_path,
        request,
        Some(settings),
        None,
        "assistant",
    )
    .await
}

pub async fn run_assistant_sidecar_jsonl<F, Fut>(
    app: &AppHandle,
    request: Value,
    settings: &AppSettings,
    mut on_event: F,
) -> Result<Value, String>
where
    F: FnMut(Value) -> Fut,
    Fut: Future<Output = ()>,
{
    let runtime = resolve_asr_runtime(app)?;
    let script_path = resolve_assistant_sidecar_script(app)?;
    run_python_sidecar_jsonl(runtime, script_path, request, settings, &mut on_event).await
}

async fn run_python_sidecar_with_cancel(
    runtime: AsrRuntime,
    script_path: PathBuf,
    request: Value,
    settings: Option<&AppSettings>,
    mut cancel: Option<tokio::sync::oneshot::Receiver<()>>,
    label: &str,
) -> Result<Value, String> {
    let request_type = request
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string();
    let request_id = request
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string();
    let mut sidecar_paths = runtime_path_entries(&runtime.root);
    if let Some(existing_path) = std::env::var_os("PATH") {
        sidecar_paths.extend(std::env::split_paths(&existing_path));
    }
    let sidecar_path = std::env::join_paths(sidecar_paths)
        .map_err(|e| format!("Failed to build {label} sidecar PATH: {e}"))?;

    let mut command = tokio::process::Command::new(&runtime.python_path);
    command
        .env("VOICE_VIBE_ASR_RUNTIME", &runtime.root)
        .env("PATH", sidecar_path)
        .arg(&script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(settings) = settings {
        let proxy_env = proxy_env_for_settings(settings);
        log::info!(
            "Starting {} sidecar with proxy env: http_proxy={}, https_proxy={}, all_proxy={}, ddgs_proxy={}",
            label,
            proxy_env
                .http_proxy
                .as_deref()
                .map(mask_proxy)
                .unwrap_or("unset".to_string()),
            proxy_env
                .https_proxy
                .as_deref()
                .map(mask_proxy)
                .unwrap_or("unset".to_string()),
            proxy_env
                .all_proxy
                .as_deref()
                .map(mask_proxy)
                .unwrap_or("unset".to_string()),
            proxy_env
                .ddgs_proxy
                .as_deref()
                .map(mask_proxy)
                .unwrap_or("unset".to_string())
        );
        apply_proxy_env(&mut command, &proxy_env);
    }
    log::info!(
        "Starting {} sidecar request: type={} id={} script={}",
        label,
        request_type,
        request_id,
        script_path.display()
    );
    let started_at = Instant::now();
    let mut child = command.spawn().map_err(|e| {
        format!(
            "Failed to start bundled Python {label} sidecar at {}: {e}",
            runtime.python_path.display(),
        )
    })?;
    let child_id = child.id().unwrap_or(0);
    log::info!(
        "{} sidecar process spawned: type={} id={} pid={}",
        label,
        request_type,
        request_id,
        child_id
    );

    if let Some(mut stdin) = child.stdin.take() {
        let payload = format!("{request}\n");
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("Failed to write sidecar request: {e}"))?;
    }

    let mut stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture sidecar stdout".to_string())?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture sidecar stderr".to_string())?;
    let stdout_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        stdout_pipe.read_to_end(&mut buffer).await.map(|_| buffer)
    });
    let stderr_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        stderr_pipe.read_to_end(&mut buffer).await.map(|_| buffer)
    });

    let status = if let Some(cancel_receiver) = cancel.as_mut() {
        tokio::select! {
            status = child.wait() => {
                status.map_err(|e| format!("Failed to read sidecar status: {e}"))?
            }
            _ = cancel_receiver => {
                log::warn!(
                    "{} sidecar cancellation requested: type={} id={} pid={} elapsed_ms={}",
                    label,
                    request_type,
                    request_id,
                    child_id,
                    started_at.elapsed().as_millis()
                );
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err("MODEL_DOWNLOAD_CANCELLED: Model download cancelled".to_string());
            }
        }
    } else {
        child
            .wait()
            .await
            .map_err(|e| format!("Failed to read sidecar status: {e}"))?
    };
    let stdout_bytes = stdout_task
        .await
        .map_err(|e| format!("Failed to join sidecar stdout reader: {e}"))?
        .map_err(|e| format!("Failed to read sidecar stdout: {e}"))?;
    let stderr_bytes = stderr_task
        .await
        .map_err(|e| format!("Failed to join sidecar stderr reader: {e}"))?
        .map_err(|e| format!("Failed to read sidecar stderr: {e}"))?;
    let stdout = String::from_utf8_lossy(&stdout_bytes);
    let stderr = String::from_utf8_lossy(&stderr_bytes);
    let stderr_text = stderr.trim();

    log::info!(
        "{} sidecar process exited: type={} id={} pid={} status={} elapsed_ms={} stdout_bytes={} stderr_bytes={}",
        label,
        request_type,
        request_id,
        child_id,
        status,
        started_at.elapsed().as_millis(),
        stdout_bytes.len(),
        stderr_bytes.len()
    );
    if !stderr_text.is_empty() {
        log::warn!(
            "{} sidecar stderr: type={} id={} {}",
            label,
            request_type,
            request_id,
            truncate_for_log(stderr_text, 1_200)
        );
    }

    if !status.success() {
        if let Ok(value) = parse_sidecar_json(&stdout) {
            if let Some(error) = sidecar_error_message(&value) {
                return Err(error);
            }
        }

        let stdout_text = stdout.trim();
        let details = match (stdout_text.is_empty(), stderr_text.is_empty()) {
            (false, false) => format!("stdout={stdout_text}; stderr={stderr_text}"),
            (false, true) => stdout_text.to_string(),
            (true, false) => stderr_text.to_string(),
            (true, true) => "no output".to_string(),
        };
        return Err(format!(
            "Sidecar exited with status {}: {}",
            status, details
        ));
    }

    let value = parse_sidecar_json(&stdout)?;

    if let Some(error) = sidecar_error_message(&value) {
        return Err(error);
    }

    log::info!(
        "{} sidecar response parsed: type={} id={} elapsed_ms={}",
        label,
        request_type,
        request_id,
        started_at.elapsed().as_millis()
    );
    Ok(value)
}

async fn run_python_sidecar_jsonl<F, Fut>(
    runtime: AsrRuntime,
    script_path: PathBuf,
    request: Value,
    settings: &AppSettings,
    on_event: &mut F,
) -> Result<Value, String>
where
    F: FnMut(Value) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut sidecar_paths = runtime_path_entries(&runtime.root);
    if let Some(existing_path) = std::env::var_os("PATH") {
        sidecar_paths.extend(std::env::split_paths(&existing_path));
    }
    let sidecar_path = std::env::join_paths(sidecar_paths)
        .map_err(|e| format!("Failed to build assistant sidecar PATH: {e}"))?;

    let mut command = tokio::process::Command::new(&runtime.python_path);
    command
        .env("VOICE_VIBE_ASR_RUNTIME", &runtime.root)
        .env("PATH", sidecar_path)
        .arg(&script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let proxy_env = proxy_env_for_settings(settings);
    log::info!(
        "Starting assistant sidecar JSONL stream with proxy env: http_proxy={}, https_proxy={}, all_proxy={}, ddgs_proxy={}",
        proxy_env
            .http_proxy
            .as_deref()
            .map(mask_proxy)
            .unwrap_or("unset".to_string()),
        proxy_env
            .https_proxy
            .as_deref()
            .map(mask_proxy)
            .unwrap_or("unset".to_string()),
        proxy_env
            .all_proxy
            .as_deref()
            .map(mask_proxy)
            .unwrap_or("unset".to_string()),
        proxy_env
            .ddgs_proxy
            .as_deref()
            .map(mask_proxy)
            .unwrap_or("unset".to_string())
    );
    apply_proxy_env(&mut command, &proxy_env);

    let mut child = command.spawn().map_err(|e| {
        format!(
            "Failed to start bundled Python assistant sidecar at {}: {e}",
            runtime.python_path.display(),
        )
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        let payload = format!("{request}\n");
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("Failed to write sidecar request: {e}"))?;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture sidecar stdout".to_string())?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture sidecar stderr".to_string())?;
    let stderr_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        stderr_pipe.read_to_end(&mut buffer).await.map(|_| buffer)
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut final_value: Option<Value> = None;
    let mut stdout_lines = Vec::new();
    loop {
        let line = lines
            .next_line()
            .await
            .map_err(|e| format!("Failed to read sidecar stream: {e}"))?;
        let Some(line) = line else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        stdout_lines.push(trimmed.to_string());
        if !trimmed.starts_with('{') {
            continue;
        }
        let value: Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("Invalid sidecar JSONL event: {e}; line={trimmed}"))?;
        if value.get("event").is_some() {
            on_event(value).await;
        } else {
            final_value = Some(value);
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to read sidecar status: {e}"))?;
    let stderr_bytes = stderr_task
        .await
        .map_err(|e| format!("Failed to join sidecar stderr reader: {e}"))?
        .map_err(|e| format!("Failed to read sidecar stderr: {e}"))?;
    let stderr = String::from_utf8_lossy(&stderr_bytes);

    if !status.success() {
        if let Some(value) = final_value.as_ref() {
            if let Some(error) = sidecar_error_message(value) {
                return Err(error);
            }
        }
        let stdout_text = stdout_lines.join("\n");
        let stdout_text = stdout_text.trim();
        let stderr_text = stderr.trim();
        let details = match (stdout_text.is_empty(), stderr_text.is_empty()) {
            (false, false) => format!("stdout={stdout_text}; stderr={stderr_text}"),
            (false, true) => stdout_text.to_string(),
            (true, false) => stderr_text.to_string(),
            (true, true) => "no output".to_string(),
        };
        return Err(format!(
            "Sidecar exited with status {}: {}",
            status, details
        ));
    }

    let value = final_value.ok_or_else(|| {
        format!(
            "Sidecar returned no final JSON response. stdout={}",
            stdout_lines.join("\n")
        )
    })?;
    if let Some(error) = sidecar_error_message(&value) {
        return Err(error);
    }
    Ok(value)
}

#[derive(Debug, Default)]
struct ProxyEnv {
    http_proxy: Option<String>,
    https_proxy: Option<String>,
    all_proxy: Option<String>,
    ddgs_proxy: Option<String>,
}

fn proxy_env_for_settings(settings: &AppSettings) -> ProxyEnv {
    let configured_http = configured_proxy(settings.http_proxy.as_deref());
    let configured_https = configured_proxy(settings.https_proxy.as_deref());
    let configured_all = configured_proxy(settings.all_proxy.as_deref());

    let http_proxy = configured_http.clone();
    let https_proxy = configured_https.clone().or_else(|| configured_http.clone());
    let all_proxy = configured_all.clone();
    let ddgs_proxy = configured_https
        .clone()
        .or_else(|| configured_http.clone())
        .or_else(|| configured_all.clone());

    ProxyEnv {
        http_proxy,
        https_proxy,
        all_proxy,
        ddgs_proxy,
    }
}

fn configured_proxy(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn apply_proxy_env(command: &mut tokio::process::Command, proxy_env: &ProxyEnv) {
    set_proxy_env(command, "http_proxy", proxy_env.http_proxy.as_deref());
    set_proxy_env(command, "HTTP_PROXY", proxy_env.http_proxy.as_deref());
    set_proxy_env(command, "https_proxy", proxy_env.https_proxy.as_deref());
    set_proxy_env(command, "HTTPS_PROXY", proxy_env.https_proxy.as_deref());
    set_proxy_env(command, "ALL_PROXY", proxy_env.all_proxy.as_deref());
    set_proxy_env(command, "all_proxy", proxy_env.all_proxy.as_deref());
    set_proxy_env(command, "DDGS_PROXY", proxy_env.ddgs_proxy.as_deref());
}

fn set_proxy_env(command: &mut tokio::process::Command, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        command.env(key, value);
    } else {
        command.env_remove(key);
    }
}

fn mask_proxy(proxy: &str) -> String {
    let Some((scheme, rest)) = proxy.split_once("://") else {
        return "set".to_string();
    };
    let host = rest
        .rsplit('@')
        .next()
        .unwrap_or(rest)
        .split('/')
        .next()
        .unwrap_or(rest);
    format!("{scheme}://{host}")
}

fn parse_sidecar_json(stdout: &str) -> Result<Value, String> {
    let final_line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .ok_or_else(|| {
            format!(
                "Sidecar returned no JSON response. stdout={}",
                stdout.trim()
            )
        })?;
    serde_json::from_str(final_line)
        .map_err(|e| format!("Invalid sidecar JSON response: {e}; line={final_line}"))
}

fn truncate_for_log(value: &str, max_chars: usize) -> String {
    let mut truncated = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            truncated.push_str("...");
            return truncated;
        }
        truncated.push(ch);
    }
    truncated
}

fn sidecar_error_message(value: &Value) -> Option<String> {
    if value.get("ok").and_then(|ok| ok.as_bool()) != Some(false) {
        return None;
    }
    let code = value
        .pointer("/error/code")
        .and_then(|v| v.as_str())
        .unwrap_or("SIDECAR_ERROR");
    let message = value
        .pointer("/error/message")
        .and_then(|v| v.as_str())
        .unwrap_or("Python sidecar failed");
    Some(format!("{code}: {message}"))
}

pub async fn runtime_dependency_status(app: &AppHandle) -> (bool, String, bool, String) {
    match resolve_asr_runtime(app) {
        Ok(runtime) => {
            let (python_ok, python_message) =
                command_version_path(&runtime.python_path, "--version").await;
            let (ffmpeg_ok, ffmpeg_message) =
                command_version_path(&runtime.ffmpeg_path, "-version").await;
            (python_ok, python_message, ffmpeg_ok, ffmpeg_message)
        }
        Err(error) => (false, error.clone(), false, error),
    }
}

pub fn runtime_path_entries(runtime_root: &std::path::Path) -> Vec<PathBuf> {
    let mut paths = vec![runtime_root.join("bin")];
    if cfg!(windows) {
        paths.push(runtime_root.to_path_buf());
        paths.push(runtime_root.join("Scripts"));
    }
    paths
}

fn bundled_python_relative_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("python.exe")
    } else {
        PathBuf::from("bin").join("python")
    }
}

fn bundled_ffmpeg_relative_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("bin").join("ffmpeg.exe")
    } else {
        PathBuf::from("bin").join("ffmpeg")
    }
}

async fn command_version_path(command: &PathBuf, arg: &str) -> (bool, String) {
    match tokio::process::Command::new(command)
        .arg(arg)
        .output()
        .await
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let message = if stdout.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                stdout.lines().next().unwrap_or("").trim().to_string()
            };
            (output.status.success(), message)
        }
        Err(error) => (false, format!("{}: {error}", command.display())),
    }
}

pub fn prepare_model_request(id: &str, repo: &str, target_dir: &str, source: &str) -> Value {
    json!({
        "id": id,
        "type": "prepare_model",
        "payload": {
            "repo": repo,
            "target_dir": target_dir,
            "source": source
        }
    })
}

pub fn prepare_auxiliary_models_request(id: &str, target_dir: &str, source: &str) -> Value {
    json!({
        "id": id,
        "type": "prepare_auxiliary_models",
        "payload": {
            "target_dir": target_dir,
            "source": source
        }
    })
}

pub fn prepare_punctuation_model_request(id: &str, target_dir: &str, source: &str) -> Value {
    json!({
        "id": id,
        "type": "prepare_punctuation_model",
        "payload": {
            "target_dir": target_dir,
            "source": source
        }
    })
}

pub fn prepare_dictation_models_request(
    id: &str,
    repo: &str,
    target_dir: &str,
    source: &str,
) -> Value {
    json!({
        "id": id,
        "type": "prepare_dictation_models",
        "payload": {
            "repo": repo,
            "target_dir": target_dir,
            "source": source
        }
    })
}

pub fn estimate_model_download_size_request(
    id: &str,
    repo: &str,
    source: &str,
    profile: &str,
) -> Value {
    json!({
        "id": id,
        "type": "estimate_model_download_size",
        "payload": {
            "repo": repo,
            "source": source,
            "profile": profile
        }
    })
}

pub fn transcribe_request(
    id: &str,
    audio_path: &str,
    normalized_path: &str,
    model_path: &str,
    ffmpeg_path: &str,
    use_gpu: bool,
    vad_model_path: &str,
    speaker_model_path: &str,
    punc_model_path: &str,
) -> Value {
    transcribe_request_with_profile(
        id,
        audio_path,
        normalized_path,
        model_path,
        ffmpeg_path,
        use_gpu,
        Some(vad_model_path),
        Some(speaker_model_path),
        Some(punc_model_path),
        "meeting_full",
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn transcribe_request_with_profile(
    id: &str,
    audio_path: &str,
    normalized_path: &str,
    model_path: &str,
    ffmpeg_path: &str,
    use_gpu: bool,
    vad_model_path: Option<&str>,
    speaker_model_path: Option<&str>,
    punc_model_path: Option<&str>,
    profile: &str,
    audio_already_normalized: bool,
) -> Value {
    let mut request = json!({
        "id": id,
        "type": "transcribe",
        "payload": {
            "audio_path": audio_path,
            "normalized_path": normalized_path,
            "model_path": model_path,
            "ffmpeg_path": ffmpeg_path,
            "use_gpu": use_gpu,
            "profile": profile,
            "audio_already_normalized": audio_already_normalized
        }
    });
    if let Some(payload) = request.get_mut("payload").and_then(Value::as_object_mut) {
        if let Some(path) = vad_model_path {
            payload.insert(
                "vad_model_path".to_string(),
                Value::String(path.to_string()),
            );
        }
        if let Some(path) = speaker_model_path {
            payload.insert(
                "speaker_model_path".to_string(),
                Value::String(path.to_string()),
            );
        }
        if let Some(path) = punc_model_path {
            payload.insert(
                "punc_model_path".to_string(),
                Value::String(path.to_string()),
            );
        }
    }
    request
}

fn validate_asr_runtime(runtime: &AsrRuntime) -> Result<(), String> {
    let required = [
        ("ASR runtime root", &runtime.root),
        ("bundled Python", &runtime.python_path),
        ("bundled ffmpeg", &runtime.ffmpeg_path),
        ("FunASR workflow sidecar", &runtime.script_path),
    ];
    for (label, path) in required {
        if !path.exists() {
            return Err(format!("{label} missing: {}", path.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictation_transcribe_request_marks_normalized_audio_and_profile() {
        let request = transcribe_request_with_profile(
            "dictation-1",
            "/tmp/input.wav",
            "/tmp/normalized.wav",
            "/tmp/model",
            "/tmp/ffmpeg",
            true,
            None,
            None,
            Some("/tmp/punc"),
            "dictation",
            true,
        );

        assert_eq!(request["type"], "transcribe");
        let payload = &request["payload"];
        assert_eq!(payload["profile"], "dictation");
        assert_eq!(payload["audio_already_normalized"], true);
        assert_eq!(payload["punc_model_path"], "/tmp/punc");
        assert!(payload.get("vad_model_path").is_none());
        assert!(payload.get("speaker_model_path").is_none());
    }

    #[test]
    fn dictation_model_prepare_request_uses_dedicated_sidecar_type() {
        let request = prepare_dictation_models_request(
            "prepare-1",
            "paraformer-zh",
            "/tmp/asr",
            "modelscope",
        );

        assert_eq!(request["type"], "prepare_dictation_models");
        assert_eq!(request["payload"]["repo"], "paraformer-zh");
        assert_eq!(request["payload"]["target_dir"], "/tmp/asr");
        assert_eq!(request["payload"]["source"], "modelscope");
    }

    #[test]
    fn punctuation_model_prepare_request_uses_dedicated_sidecar_type() {
        let request = prepare_punctuation_model_request("punc-1", "/tmp/aux", "modelscope");

        assert_eq!(request["type"], "prepare_punctuation_model");
        assert_eq!(request["payload"]["target_dir"], "/tmp/aux");
        assert_eq!(request["payload"]["source"], "modelscope");
    }

    #[test]
    fn estimate_model_download_size_request_includes_profile() {
        let request = estimate_model_download_size_request(
            "estimate-1",
            "paraformer-zh",
            "huggingface",
            "dictation",
        );

        assert_eq!(request["type"], "estimate_model_download_size");
        assert_eq!(request["payload"]["repo"], "paraformer-zh");
        assert_eq!(request["payload"]["source"], "huggingface");
        assert_eq!(request["payload"]["profile"], "dictation");
    }
}
