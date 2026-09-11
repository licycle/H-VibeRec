//! Audio is spooled before submission; the queue holds metadata, not recordings
//! or heavyweight model instances. One ASR lane feeds independent polish jobs.
use crate::{
    db,
    pastebox::DictationContext,
    types::{AppSettings, VoiceInputSubmission},
};
use std::{path::PathBuf, sync::OnceLock};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Semaphore};

pub(super) static POLISH_SLOTS: Semaphore = Semaphore::const_new(2);
static QUEUE: OnceLock<mpsc::UnboundedSender<(AppHandle, DictationJob)>> = OnceLock::new();

// Only the opt-in, isolated debug audit can install per-UUID responses.
// Release builds contain no injection path or environment-controlled bypass.
#[cfg(all(debug_assertions, target_os = "macos"))]
mod audit_hooks {
    use std::{
        collections::HashMap,
        sync::{LazyLock, Mutex},
    };
    use tokio::sync::oneshot;
    type Response = Result<String, String>;
    static RESPONSES: LazyLock<Mutex<HashMap<(String, String), oneshot::Receiver<Response>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    pub fn block(id: &str, stage: &str) -> oneshot::Sender<Response> {
        let (sender, receiver) = oneshot::channel();
        RESPONSES
            .lock()
            .unwrap()
            .insert((id.into(), stage.into()), receiver);
        sender
    }
    pub async fn take(id: &str, stage: &str) -> Option<Response> {
        let response = RESPONSES.lock().unwrap().remove(&(id.into(), stage.into()));
        match response {
            Some(receiver) => Some(
                receiver
                    .await
                    .unwrap_or_else(|_| Err("Audit response dropped".into())),
            ),
            None => None,
        }
    }
}
#[cfg(all(debug_assertions, target_os = "macos"))]
pub(crate) use audit_hooks::{block as audit_block, take as audit_response};

pub(super) struct DictationJob {
    pub id: String,
    pub settings: AppSettings,
    pub context: Option<DictationContext>,
}

pub(crate) fn paths(id: &str) -> Result<(PathBuf, PathBuf), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "无效的录音任务编号")?;
    let dir = crate::storage::get_temp_dir()?.join("voice-input");
    Ok((
        dir.join(format!("{id}.wav")),
        dir.join(format!("{id}.normalized.wav")),
    ))
}

pub(super) async fn save(
    context: &DictationContext,
    settings: AppSettings,
    samples: Vec<f32>,
) -> Result<DictationJob, String> {
    let id = context.id().to_string();
    let (audio, _) = paths(&id)?;
    let path = audio.clone();
    tauri::async_runtime::spawn_blocking(move || super::recorder::write_wav(&path, &samples))
        .await
        .map_err(|e| e.to_string())??;
    if let Err(error) = db::insert_dictation_job(&id, context.mode()) {
        return Err(format!(
            "无法登记任务：{error}；录音保存在 {}",
            audio.display()
        ));
    }
    Ok(DictationJob {
        id,
        settings,
        context: Some(context.clone()),
    })
}

pub(super) fn submit(app: AppHandle, job: DictationJob) -> Result<(), String> {
    // Persist the target as soon as capture finishes, including when ASR fails.
    if let Some(context) = job.context.clone() {
        let id = job.id.clone();
        let capture_app = app.clone();
        tauri::async_runtime::spawn(async move {
            let (_, target) = context.resolve().await;
            let _ = db::set_dictation_target(&id, target.as_ref());
            crate::pastebox::changed(&capture_app).await;
        });
    }
    let queue = QUEUE.get_or_init(|| {
        let (sender, receiver) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(run(receiver));
        sender
    });
    queue.send((app, job)).map_err(|error| {
        let (app, job) = error.0;
        let message = "后台队列不可用，录音已保留，请重试";
        let _ = db::fail_dictation_job(&job.id, message);
        let _ = app.emit("pastebox-changed", ());
        message.to_string()
    })
}

async fn run(mut receiver: mpsc::UnboundedReceiver<(AppHandle, DictationJob)>) {
    while let Some((app, job)) = receiver.recv().await {
        let id = job.id.clone();
        match super::process_samples(&app, &job).await {
            Ok(raw) => {
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = super::finish_samples(app.clone(), job, raw).await {
                        failed(&app, &id, &error).await;
                    }
                });
            }
            Err(error) => failed(&app, &id, &error).await,
        }
    }
}

pub(super) async fn stage(app: &AppHandle, id: &str, stage: &str) -> Result<(), String> {
    db::set_dictation_stage(id, stage)?;
    crate::pastebox::changed(app).await;
    Ok(())
}

async fn failed(app: &AppHandle, id: &str, error: &str) {
    log::error!("Dictation background failed: job={id} error={error}");
    // A failure after ASR keeps usable text. Only pre-transcription failures
    // expose audio retry, which never reruns an already attempted delivery.
    if let Ok(item) = db::get_paste_item(id) {
        if item.raw_text.is_empty() {
            let _ = db::fail_dictation_job(id, error);
        } else {
            let _ = db::fail_dictation_completion(id, error);
        }
        super::show_navigation_feedback(app, &format!("#{} 处理未完成，请查看粘贴箱", item.seq));
    }
    crate::pastebox::changed(app).await;
}

pub(crate) async fn retry(
    app: AppHandle,
    id: &str,
    version: i64,
) -> Result<VoiceInputSubmission, String> {
    let (audio, _) = paths(id)?;
    if !audio.is_file() {
        return Err("保留的录音文件不存在，无法重试".into());
    }
    let settings = db::get_runtime_settings()?;
    let item = db::retry_dictation_job(id, version)?;
    submit(
        app.clone(),
        DictationJob {
            id: id.into(),
            settings,
            context: None,
        },
    )?;
    crate::pastebox::changed(&app).await;
    Ok(VoiceInputSubmission {
        job_id: id.into(),
        seq: item.seq,
        phase: "queued".into(),
    })
}
