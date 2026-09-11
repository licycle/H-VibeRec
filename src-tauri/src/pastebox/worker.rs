//! Persistent PyObjC client. AX references never cross the process boundary.
use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tauri::{AppHandle, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};

static WORKER: Mutex<Option<Worker>> = Mutex::const_new(None);

struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

fn paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let root = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    } else {
        app.path().resource_dir().map_err(|e| e.to_string())?
    };
    let python = root.join("runtime/asr/bin/python3");
    // A development-only override permits testing AX independently of downloading ASR models.
    #[cfg(debug_assertions)]
    let python = std::env::var_os("HVR_AX_PYTHON")
        .map(PathBuf::from)
        .unwrap_or(python);
    let script = root.join("sidecars/pastebox_ax/main.py");
    if !python.is_file() || !script.is_file() {
        return Err("辅助功能运行时缺失，请运行 npm run runtime:ensure 后重新启动".into());
    }
    Ok((python, script))
}

impl Worker {
    fn spawn(app: &AppHandle) -> Result<Self, String> {
        let (python, script) = paths(app)?;
        let mut child = Command::new(python)
            .args(["-u", "-B", "-s"])
            .arg(script)
            .env("HVR_OWNER_PID", std::process::id().to_string())
            .env("PYTHONNOUSERSITE", "1")
            .env_remove("PYTHONPATH")
            .env_remove("PYTHONHOME")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("无法启动 PyObjC 辅助功能服务：{e}"))?;
        let stdin = child.stdin.take().ok_or("辅助功能输入管道不可用")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("辅助功能输出管道不可用")?);
        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    async fn request(&mut self, mut request: Value) -> Result<Value, String> {
        let is_paste = request["op"] == "paste";
        let id = uuid::Uuid::new_v4().to_string();
        request["request_id"] = json!(id);
        let mut bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(|e| e.to_string())?;
        self.stdin.flush().await.map_err(|e| e.to_string())?;
        let mut line = String::new();
        if self
            .stdout
            .read_line(&mut line)
            .await
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err("辅助功能服务已退出，请检查 PyObjC 运行时".into());
        }
        let response: Value =
            serde_json::from_str(&line).map_err(|_| "辅助功能服务返回了无效响应".to_string())?;
        if response["request_id"] != id {
            return Err("辅助功能响应编号不匹配".into());
        }
        if !response["error"].is_string()
            && (!response["result"].is_object()
                || (is_paste
                    && (!response["result"]["verified"].is_boolean()
                        || !response["result"]["message"].is_string())))
        {
            return Err("辅助功能响应缺少必要的结果字段".into());
        }
        Ok(response)
    }
}

pub async fn call(app: &AppHandle, request: Value) -> Result<Value, String> {
    let operation = request["op"].as_str().unwrap_or("unknown").to_string();
    let started = std::time::Instant::now();
    let mut slot = if request["op"] == "capture" {
        // A brief status request from another window must not discard the dictation target.
        // Keep this bounded; expected_pid and AX focus checks reject a changed destination.
        tokio::time::timeout(Duration::from_millis(800), WORKER.lock())
            .await
            .map_err(|_| "辅助功能正在处理其他操作，请稍后重新记录位置".to_string())?
    } else {
        WORKER.lock().await
    };
    let is_paste = request["op"] == "paste";
    // Taking ownership makes cancellation kill the process; no late response can reach a new job.
    let mut worker = match slot.take() {
        Some(worker) => worker,
        None => Worker::spawn(app)?,
    };
    let response = tokio::time::timeout(Duration::from_secs(10), worker.request(request)).await;
    match response {
        Ok(Ok(response)) => {
            *slot = Some(worker);
            if let Some(error) = response["error"].as_str() {
                log::warn!(
                    "AX operation {operation} failed after {} ms: {error}",
                    started.elapsed().as_millis()
                );
                Err(error.to_string())
            } else {
                log::debug!(
                    "AX operation {operation} completed in {} ms",
                    started.elapsed().as_millis()
                );
                Ok(response["result"].clone())
            }
        }
        failure => {
            let _ = worker.child.kill().await;
            if is_paste {
                // A transport failure cannot prove whether input was delivered. Never replay it.
                Ok(
                    json!({"verified": false, "message": "辅助功能服务中断，粘贴结果不确定；请检查内容并重新记录位置"}),
                )
            } else {
                Err(match failure {
                    Ok(Err(error)) => error,
                    _ => "辅助功能服务响应超时，旧目标已失效".into(),
                })
            }
        }
    }
}
