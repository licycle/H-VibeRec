use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lazy_static::lazy_static;
use serde_json::Value;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

use crate::sidecar;
use crate::types::AppSettings;

const ASR_IDLE_TTL_SECS: u64 = 60;
const ASR_DICTATION_IDLE_TTL_SECS: u64 = 20 * 60;

lazy_static! {
    static ref ASR_WORKER_POOL: Arc<AsrWorkerPool> = Arc::new(AsrWorkerPool::default());
}

#[derive(Default)]
struct AsrWorkerPool {
    inner: Mutex<AsrWorkerState>,
}

#[derive(Default)]
struct AsrWorkerState {
    worker: Option<AsrWorkerProcess>,
    generation: u64,
    last_used_at: Option<Instant>,
}

struct AsrWorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub async fn transcribe(
    app: &AppHandle,
    request: Value,
    settings: &AppSettings,
) -> Result<Value, String> {
    ASR_WORKER_POOL.transcribe(app, request, settings).await
}

impl AsrWorkerPool {
    async fn transcribe(
        self: &Arc<Self>,
        app: &AppHandle,
        request: Value,
        settings: &AppSettings,
    ) -> Result<Value, String> {
        let request_id = request_id(&request);
        let profile = request_profile(&request);
        let queue_started = Instant::now();
        let mut guard = self.inner.lock().await;
        let queue_elapsed_ms = queue_started.elapsed().as_millis();
        log::info!(
            "ASR worker request dequeued: id={} profile={} queue_wait_ms={} has_worker={}",
            request_id,
            profile,
            queue_elapsed_ms,
            guard.worker.is_some()
        );
        if guard.worker.is_none() {
            let worker_start = Instant::now();
            guard.worker = Some(start_worker(app, settings).await?);
            guard.generation = guard.generation.saturating_add(1);
            log::info!(
                "ASR worker started: id={} profile={} startup_ms={} generation={}",
                request_id,
                profile,
                worker_start.elapsed().as_millis(),
                guard.generation
            );
        }

        let request_started = Instant::now();
        let result = if let Some(worker) = guard.worker.as_mut() {
            worker.send_request(&request).await
        } else {
            Err("ASR worker was not available".to_string())
        };

        match result {
            Ok(value) => {
                guard.last_used_at = Some(Instant::now());
                let generation = guard.generation;
                let idle_ttl_secs = idle_ttl_for_request(&request);
                let sidecar_total_ms = value
                    .pointer("/result/timing/total_asr_ms")
                    .or_else(|| value.pointer("/result/timing/total_warmup_ms"))
                    .and_then(|value| value.as_i64());
                let sidecar_infer_ms = value
                    .pointer("/result/timing/asr_infer_ms")
                    .or_else(|| value.pointer("/result/timing/warmup_infer_ms"))
                    .and_then(|value| value.as_i64());
                log::info!(
                    "ASR worker request completed: id={} profile={} queue_wait_ms={} worker_elapsed_ms={} idle_ttl_secs={} sidecar_total_ms={:?} sidecar_infer_ms={:?}",
                    request_id,
                    profile,
                    queue_elapsed_ms,
                    request_started.elapsed().as_millis(),
                    idle_ttl_secs,
                    sidecar_total_ms,
                    sidecar_infer_ms
                );
                drop(guard);
                self.schedule_idle_shutdown(generation, idle_ttl_secs);
                Ok(value)
            }
            Err(error) => {
                log::error!(
                    "ASR worker request failed: id={} profile={} queue_wait_ms={} worker_elapsed_ms={} error={}",
                    request_id,
                    profile,
                    queue_elapsed_ms,
                    request_started.elapsed().as_millis(),
                    error
                );
                if let Some(mut worker) = guard.worker.take() {
                    let _ = worker.child.kill().await;
                    let _ = worker.child.wait().await;
                }
                guard.generation = guard.generation.saturating_add(1);
                Err(error)
            }
        }
    }

    fn schedule_idle_shutdown(self: &Arc<Self>, generation: u64, idle_ttl_secs: u64) {
        let pool = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(idle_ttl_secs)).await;
            let mut guard = pool.inner.lock().await;
            if guard.generation != generation {
                return;
            }
            let idle_long_enough = guard
                .last_used_at
                .map(|at| at.elapsed() >= Duration::from_secs(idle_ttl_secs))
                .unwrap_or(false);
            if !idle_long_enough {
                return;
            }
            if let Some(mut worker) = guard.worker.take() {
                log::info!(
                    "ASR worker idle shutdown started: generation={} idle_ttl_secs={}",
                    generation,
                    idle_ttl_secs
                );
                let _ = worker
                    .stdin
                    .write_all(br#"{"id":"shutdown","type":"shutdown","payload":{}}"#)
                    .await;
                let _ = worker.stdin.write_all(b"\n").await;
                let _ = worker.stdin.flush().await;
                match tokio::time::timeout(Duration::from_secs(3), worker.child.wait()).await {
                    Ok(_) => {}
                    Err(_) => {
                        let _ = worker.child.kill().await;
                        let _ = worker.child.wait().await;
                    }
                }
                log::info!(
                    "ASR worker idle shutdown finished: generation={} idle_ttl_secs={}",
                    generation,
                    idle_ttl_secs
                );
            }
        });
    }
}

impl AsrWorkerProcess {
    async fn send_request(&mut self, request: &Value) -> Result<Value, String> {
        let payload = format!("{request}\n");
        self.stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("Failed to write ASR worker request: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush ASR worker request: {e}"))?;

        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self
                .stdout
                .read_line(&mut line)
                .await
                .map_err(|e| format!("Failed to read ASR worker response: {e}"))?;
            if bytes == 0 {
                return Err("ASR worker exited before returning a response".to_string());
            }
            let trimmed = line.trim();
            if !trimmed.starts_with('{') {
                continue;
            }
            let value: Value = serde_json::from_str(trimmed)
                .map_err(|e| format!("Invalid ASR worker JSON response: {e}; line={trimmed}"))?;
            if let Some(error) = worker_error_message(&value) {
                return Err(error);
            }
            return Ok(value);
        }
    }
}

async fn start_worker(app: &AppHandle, settings: &AppSettings) -> Result<AsrWorkerProcess, String> {
    let runtime = sidecar::resolve_asr_runtime(app)?;
    let mut sidecar_paths = sidecar::runtime_path_entries(&runtime.root);
    if let Some(existing_path) = std::env::var_os("PATH") {
        sidecar_paths.extend(std::env::split_paths(&existing_path));
    }
    let sidecar_path = std::env::join_paths(sidecar_paths)
        .map_err(|e| format!("Failed to build ASR worker PATH: {e}"))?;

    let mut command = tokio::process::Command::new(&runtime.python_path);
    command
        .env("VOICE_VIBE_ASR_RUNTIME", &runtime.root)
        .env("PATH", sidecar_path)
        .arg(&runtime.script_path)
        .arg("--server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_proxy_env(&mut command, settings);

    let mut child = command.spawn().map_err(|e| {
        format!(
            "Failed to start reusable ASR worker at {}: {e}",
            runtime.python_path.display()
        )
    })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Failed to capture ASR worker stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture ASR worker stdout".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture ASR worker stderr".to_string())?;
    tokio::spawn(async move {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer).await;
        if !buffer.is_empty() {
            log::info!(
                "ASR worker stderr: {}",
                String::from_utf8_lossy(&buffer).trim()
            );
        }
    });

    Ok(AsrWorkerProcess {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}

fn worker_error_message(value: &Value) -> Option<String> {
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
        .unwrap_or("Python ASR worker failed");
    Some(format!("{code}: {message}"))
}

fn request_id(request: &Value) -> String {
    request
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string()
}

fn request_profile(request: &Value) -> &'static str {
    match request
        .pointer("/payload/profile")
        .and_then(|value| value.as_str())
    {
        Some("dictation") => "dictation",
        _ => "meeting_full",
    }
}

fn idle_ttl_for_request(request: &Value) -> u64 {
    match request_profile(request) {
        "dictation" => ASR_DICTATION_IDLE_TTL_SECS,
        _ => ASR_IDLE_TTL_SECS,
    }
}

fn apply_proxy_env(command: &mut tokio::process::Command, settings: &AppSettings) {
    set_proxy_env(command, "http_proxy", settings.http_proxy.as_deref());
    set_proxy_env(command, "HTTP_PROXY", settings.http_proxy.as_deref());
    let https_proxy = settings
        .https_proxy
        .as_deref()
        .or(settings.http_proxy.as_deref());
    set_proxy_env(command, "https_proxy", https_proxy);
    set_proxy_env(command, "HTTPS_PROXY", https_proxy);
    set_proxy_env(command, "ALL_PROXY", settings.all_proxy.as_deref());
    set_proxy_env(command, "all_proxy", settings.all_proxy.as_deref());
}

fn set_proxy_env(command: &mut tokio::process::Command, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        command.env(key, value);
    } else {
        command.env_remove(key);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn dictation_requests_use_longer_idle_ttl_than_meeting_requests() {
        let dictation = json!({
            "type": "transcribe",
            "payload": {
                "profile": "dictation"
            }
        });
        let meeting = json!({
            "type": "transcribe",
            "payload": {
                "profile": "meeting_full"
            }
        });
        let legacy = json!({
            "type": "transcribe",
            "payload": {}
        });

        assert_eq!(
            super::idle_ttl_for_request(&dictation),
            super::ASR_DICTATION_IDLE_TTL_SECS
        );
        assert_eq!(
            super::idle_ttl_for_request(&meeting),
            super::ASR_IDLE_TTL_SECS
        );
        assert_eq!(
            super::idle_ttl_for_request(&legacy),
            super::ASR_IDLE_TTL_SECS
        );
    }
}
