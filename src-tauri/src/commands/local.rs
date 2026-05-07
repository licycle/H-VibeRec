use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lazy_static::lazy_static;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::db;
use crate::llm;
use crate::sidecar;
use crate::types::{
    AppSettings, LocalRecording, ModelDownloadProgress, ModelStatus, RuntimeDependencyStatus,
    SaveSettingsRequest, SummaryJobResult, SummaryRecord, SummaryTemplate, TranscriptRecord,
    TranscriptionJobResult, WorkspaceSummaryResult, WorkspaceTextDocument,
};

const FUNASR_AUXILIARY_DIR: &str = ".voice_vibe_aux";
const FUNASR_CUSTOM_AUXILIARY_DIR: &str = "FunASR-Workflow__auxiliary";
const FUNASR_WORKFLOW_DEFAULT_REPO: &str = "paraformer-zh";
const FUNASR_LEGACY_NANO_REPO: &str = "mlx-community/Fun-ASR-Nano-2512-fp16";
const DOWNLOAD_SPEED_HOLD_SECS: u64 = 12;
const DOWNLOAD_STALL_MESSAGE_SECS: u64 = 20;

lazy_static! {
    static ref MODEL_DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::new(());
    static ref ACTIVE_MODEL_DOWNLOAD: tokio::sync::Mutex<Option<ActiveModelDownload>> =
        tokio::sync::Mutex::new(None);
    static ref MODEL_DOWNLOAD_CACHE: Mutex<ModelDownloadCache> =
        Mutex::new(ModelDownloadCache::default());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveDownloadKind {
    Main,
    Auxiliary,
    Punctuation,
}

struct ActiveModelDownload {
    kind: ActiveDownloadKind,
    repo: String,
    source: String,
    target_dir: PathBuf,
    model_path: Option<PathBuf>,
    total_bytes: Option<u64>,
    cancel: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
struct ModelDownloadCache {
    snapshot: Option<ModelDownloadProgress>,
    last_sample: Option<(Instant, u64)>,
    last_nonzero_speed: f64,
    last_nonzero_at: Option<Instant>,
    last_growth_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
enum DownloadSizeProfile {
    Pipeline,
    Dictation,
    Auxiliary,
    Punctuation,
}

impl DownloadSizeProfile {
    fn as_str(self) -> &'static str {
        match self {
            DownloadSizeProfile::Pipeline => "pipeline",
            DownloadSizeProfile::Dictation => "dictation",
            DownloadSizeProfile::Auxiliary => "auxiliary",
            DownloadSizeProfile::Punctuation => "punctuation",
        }
    }
}

#[tauri::command]
pub async fn list_recordings_with_status() -> Result<Vec<LocalRecording>, String> {
    db::list_recordings()
}

#[tauri::command]
pub async fn register_recording(
    file_path: String,
    title: Option<String>,
) -> Result<LocalRecording, String> {
    db::register_recording(Path::new(&file_path), title)
}

#[tauri::command]
pub async fn delete_recording(recording_id: String) -> Result<(), String> {
    db::delete_recording(&recording_id)
}

#[tauri::command]
pub async fn get_settings() -> Result<AppSettings, String> {
    db::get_settings()
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    settings: SaveSettingsRequest,
) -> Result<AppSettings, String> {
    crate::voice_input::hotkey::parse_hotkey(&settings.voice_input_hotkey)?;
    let previous_settings = db::get_settings()?;
    crate::voice_input::apply_hotkey_registration(
        settings.voice_input_enabled,
        &settings.voice_input_hotkey,
    )?;
    match db::save_settings(settings) {
        Ok(saved) => {
            if crate::voice_input::should_schedule_dictation_warmup_after_settings_change(
                &previous_settings,
                &saved,
            ) {
                crate::voice_input::schedule_dictation_warmup(app, "settings-change", 1);
            }
            Ok(saved)
        }
        Err(error) => {
            let _ = crate::voice_input::reload_hotkey_registration();
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn set_llm_api_key(api_key: String) -> Result<(), String> {
    db::set_llm_api_key(api_key)
}

#[tauri::command]
pub async fn has_llm_api_key() -> Result<bool, String> {
    Ok(db::has_llm_api_key())
}

#[tauri::command]
pub async fn test_llm_provider() -> Result<String, String> {
    let settings = db::get_settings()?;
    let api_key = db::get_llm_api_key()?;
    llm::test_provider(&settings, &api_key).await
}

#[tauri::command]
pub async fn check_runtime_dependencies(app: AppHandle) -> Result<RuntimeDependencyStatus, String> {
    let (python_ok, python_message, ffmpeg_ok, ffmpeg_message) =
        sidecar::runtime_dependency_status(&app).await;
    Ok(RuntimeDependencyStatus {
        python_ok,
        python_message,
        ffmpeg_ok,
        ffmpeg_message,
    })
}

#[tauri::command]
pub async fn list_summary_templates() -> Result<Vec<SummaryTemplate>, String> {
    db::list_templates()
}

#[tauri::command]
pub async fn save_summary_template(
    id: Option<String>,
    name: String,
    description: Option<String>,
    prompt: String,
) -> Result<SummaryTemplate, String> {
    db::save_template(id, name, description, prompt)
}

#[tauri::command]
pub async fn delete_summary_template(id: String) -> Result<(), String> {
    db::delete_template(&id)
}

#[tauri::command]
pub async fn get_model_status() -> Result<ModelStatus, String> {
    let settings = db::get_settings()?;
    model_status_for_settings(&settings)
}

#[tauri::command]
pub async fn get_model_download_progress() -> Result<ModelDownloadProgress, String> {
    let settings = db::get_settings()?;
    let target_dir = model_download_root_for_settings(&settings)?;
    let actual_repo = resolved_asr_model_repo(&settings);
    let status = db::get_model_status(&model_status_repo_key(&settings))?;
    let downloaded_bytes = scan_dir_size(&target_dir);
    let inferred_status = if status.status == "not_downloaded" && downloaded_bytes > 0 {
        "downloading"
    } else {
        status.status.as_str()
    };
    Ok(cached_model_download_progress(
        &actual_repo,
        &settings.asr_model_source,
        inferred_status,
        downloaded_bytes,
        status.message,
    ))
}

#[tauri::command]
pub async fn ensure_asr_model(app: AppHandle) -> Result<ModelStatus, String> {
    let settings = db::get_settings()?;
    ensure_model_ready_for_queue(&app, &settings).await
}

#[tauri::command]
pub async fn cancel_asr_model_download(app: AppHandle) -> Result<ModelStatus, String> {
    let settings = db::get_settings()?;
    let repo_key = model_status_repo_key(&settings);
    let active = {
        let mut guard = ACTIVE_MODEL_DOWNLOAD.lock().await;
        guard.take()
    };
    let Some(mut active) = active else {
        emit_model_download_progress(
            &app,
            &resolved_asr_model_repo(&settings),
            &settings.asr_model_source,
            "cancelled",
            scan_dir_size(&model_download_root_for_settings(&settings)?),
            Some("No active FunASR workflow model download".to_string()),
        );
        return model_status_for_settings(&settings);
    };

    if let Some(cancel) = active.cancel.take() {
        let _ = cancel.send(());
    }
    let downloaded_bytes = scan_dir_size(&active.target_dir);
    emit_model_download_progress_with_total(
        &app,
        &active.repo,
        &active.source,
        "cancelled",
        downloaded_bytes,
        active.total_bytes,
        Some("FunASR workflow model download cancelled".to_string()),
    );
    let mut status = db::save_model_status(ModelStatus {
        repo: repo_key,
        status: "cancelled".to_string(),
        path: active
            .model_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
        message: Some("FunASR workflow model download cancelled".to_string()),
        updated_at: None,
    })?;
    status.repo = active.repo;
    Ok(status)
}

#[tauri::command]
pub async fn transcribe_recording(
    app: AppHandle,
    recording_id: String,
) -> Result<TranscriptionJobResult, String> {
    let settings = db::get_settings()?;
    let recording = db::get_recording(&recording_id)?;
    let model = ensure_model_ready_for_queue(&app, &settings).await?;
    let model_path = model
        .path
        .clone()
        .ok_or_else(|| "ASR model is not ready".to_string())?;
    let (_auxiliary_root, vad_model_path, speaker_model_path, punc_model_path) =
        auxiliary_model_paths_for_queue(&settings, &model_path)?;
    if !auxiliary_models_ready(&settings, &model_path)? {
        return Err(
            "ASR workflow models are not ready; use 下载/检查 FunASR workflow first".to_string(),
        );
    }

    let runtime = sidecar::resolve_asr_runtime(&app)?;
    let normalized_path = db::normalized_audio_dir()?.join(format!("{}.wav", recording.id));
    let job_id = db::create_transcription_job(&recording.id, Some(&model_path))?;
    let request = sidecar::transcribe_request(
        &job_id,
        &recording.original_audio_path,
        &normalized_path.to_string_lossy(),
        &model_path,
        &runtime.ffmpeg_path.to_string_lossy(),
        settings.use_gpu,
        &vad_model_path.to_string_lossy(),
        &speaker_model_path.to_string_lossy(),
        &punc_model_path.to_string_lossy(),
    );

    let response = match sidecar::run_sidecar(&app, request, Some(&settings)).await {
        Ok(value) => value,
        Err(error) => {
            let _ = db::fail_transcription_job(&job_id, "ASR_FAILED", &error);
            return Err(error);
        }
    };

    let result = response
        .get("result")
        .cloned()
        .ok_or_else(|| "Sidecar response missing result".to_string())?;
    let text = result
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        let error = "ASR returned empty transcript".to_string();
        let _ = db::fail_transcription_job(&job_id, "EMPTY_TRANSCRIPT", &error);
        return Err(error);
    }

    let duration_ms = result
        .get("duration_seconds")
        .and_then(|value| value.as_f64())
        .map(|value| (value * 1000.0) as i64);
    let transcript = db::insert_transcript(
        &recording.id,
        &job_id,
        &text,
        &result.to_string(),
        result
            .get("language")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        result.get("confidence").and_then(|value| value.as_f64()),
        duration_ms,
        result.get("rtf").and_then(|value| value.as_f64()),
    )?;

    db::update_recording_normalized_path(&recording.id, &normalized_path, duration_ms)?;
    write_transcript_files_for_queue(&transcript)?;
    db::finish_transcription_job(&job_id)?;
    Ok(TranscriptionJobResult { job_id, transcript })
}

#[tauri::command]
pub async fn retry_transcription(
    app: AppHandle,
    recording_id: String,
) -> Result<TranscriptionJobResult, String> {
    transcribe_recording(app, recording_id).await
}

#[tauri::command]
pub async fn get_transcript(transcript_id: String) -> Result<TranscriptRecord, String> {
    db::get_transcript(&transcript_id)
}

#[tauri::command]
pub async fn get_latest_transcript(recording_id: String) -> Result<TranscriptRecord, String> {
    db::latest_transcript_for_recording(&recording_id)
}

#[tauri::command]
pub async fn summarize_transcript(
    transcript_id: String,
    template_id: String,
) -> Result<SummaryJobResult, String> {
    let settings = db::get_settings()?;
    let api_key = db::get_llm_api_key()?;
    let transcript = db::get_transcript(&transcript_id)?;
    let template = db::get_template(&template_id)?;
    let job_id = db::create_summary_job(
        &transcript.id,
        &template.id,
        &settings.llm_provider,
        &settings.llm_model,
    )?;

    let (content, raw) = match llm::summarize_transcript_text(
        &transcript.text,
        &template.prompt,
        &settings,
        &api_key,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            let _ = db::fail_summary_job(&job_id, "LLM_FAILED", &error);
            return Err(error);
        }
    };

    let summary = db::insert_summary(
        Some(&transcript.id),
        &job_id,
        &template.id,
        Some(template.name),
        &content,
        Some(raw.to_string()),
        &settings.llm_provider,
        &settings.llm_model,
    )?;
    write_summary_files_for_queue(&summary)?;
    db::finish_summary_job(&job_id)?;
    Ok(SummaryJobResult { job_id, summary })
}

#[tauri::command]
pub async fn summarize_workspace_transcripts(
    transcript_ids: Vec<String>,
    template_id: String,
    workspace_title: String,
    notes: Vec<String>,
) -> Result<WorkspaceSummaryResult, String> {
    if transcript_ids.is_empty() {
        return Err("当前工作区没有可总结的转录".to_string());
    }

    let settings = db::get_settings()?;
    let api_key = db::get_llm_api_key()?;
    let template = db::get_template(&template_id)?;
    let mut transcript_blocks = Vec::new();
    let mut first_transcript: Option<TranscriptRecord> = None;

    for transcript_id in transcript_ids {
        let transcript = db::get_transcript(&transcript_id)?;
        let recording = db::get_recording(&transcript.recording_id)?;
        if first_transcript.is_none() {
            first_transcript = Some(transcript.clone());
        }
        transcript_blocks.push(format!(
            "## 录音：{}\n\n{}",
            recording.title.trim(),
            transcript.text.trim()
        ));
    }

    let note_blocks = notes
        .into_iter()
        .map(|note| note.trim().to_string())
        .filter(|note| !note.is_empty())
        .collect::<Vec<_>>();
    let mut material = format!(
        "# 工作区：{}\n\n# 转录材料\n\n{}",
        workspace_title.trim(),
        transcript_blocks.join("\n\n---\n\n")
    );
    if !note_blocks.is_empty() {
        material.push_str("\n\n# 工作区笔记\n\n");
        material.push_str(&note_blocks.join("\n\n---\n\n"));
    }

    let workspace_template = format!(
        "{}\n\n请将以下同一工作目录下的全部转录和笔记作为一个整体处理，输出一份综合 Markdown 结果。需要跨文件合并重复信息，按主题/决策/行动项/风险归纳，不要逐文件简单罗列。",
        template.prompt
    );
    let first_transcript =
        first_transcript.ok_or_else(|| "当前工作区没有可总结的转录".to_string())?;
    let job_id = db::create_summary_job(
        &first_transcript.id,
        &template.id,
        &settings.llm_provider,
        &settings.llm_model,
    )?;

    let (content, raw) =
        match llm::summarize_transcript_text(&material, &workspace_template, &settings, &api_key)
            .await
        {
            Ok(value) => value,
            Err(error) => {
                let _ = db::fail_summary_job(&job_id, "LLM_FAILED", &error);
                return Err(error);
            }
        };

    let summary = db::insert_summary(
        Some(&first_transcript.id),
        &job_id,
        &template.id,
        Some(format!("{} 综合总结", workspace_title.trim())),
        &content,
        Some(
            json!({
                "strategy": "workspace_comprehensive_summary",
                "raw": raw
            })
            .to_string(),
        ),
        &settings.llm_provider,
        &settings.llm_model,
    )?;
    write_summary_files_for_queue(&summary)?;
    db::finish_summary_job(&job_id)?;
    Ok(WorkspaceSummaryResult {
        job_id,
        content,
        summary,
    })
}

#[tauri::command]
pub async fn summarize_workspace_texts(
    app: AppHandle,
    template_id: String,
    workspace_title: String,
    documents: Vec<WorkspaceTextDocument>,
) -> Result<crate::types::LocalQueueJob, String> {
    let text_blocks = documents
        .iter()
        .filter(|document| !document.content.trim().is_empty())
        .collect::<Vec<_>>();
    if text_blocks.is_empty() {
        return Err("当前本地空间没有可总结的文本文件".to_string());
    }
    let template = db::get_template(&template_id)?;
    crate::local_queue::enqueue_workspace_text_summary(
        &app,
        template_id,
        None,
        workspace_title.clone(),
        documents,
        format!(
            "{} 本地空间总结 - {}",
            workspace_title.trim(),
            template.name
        ),
        "workspace_text".to_string(),
    )
    .await
}

#[tauri::command]
pub async fn retry_summary(
    transcript_id: String,
    template_id: String,
) -> Result<SummaryJobResult, String> {
    summarize_transcript(transcript_id, template_id).await
}

#[tauri::command]
pub async fn get_summary(summary_id: String) -> Result<SummaryRecord, String> {
    db::get_summary(&summary_id)
}

#[tauri::command]
pub async fn enqueue_transcription(
    app: AppHandle,
    recording_id: String,
    workspace_folder: Option<String>,
    next_summary_template_id: Option<String>,
) -> Result<crate::types::LocalQueueJob, String> {
    crate::local_queue::enqueue_transcription(
        &app,
        recording_id,
        workspace_folder,
        next_summary_template_id,
    )
    .await
}

#[tauri::command]
pub async fn enqueue_summary(
    app: AppHandle,
    transcript_id: String,
    template_id: String,
    workspace_folder: Option<String>,
) -> Result<crate::types::LocalQueueJob, String> {
    crate::local_queue::enqueue_summary(&app, transcript_id, template_id, workspace_folder).await
}

#[tauri::command]
pub async fn enqueue_workspace_summary(
    app: AppHandle,
    transcript_ids: Vec<String>,
    template_id: String,
    workspace_folder: Option<String>,
    workspace_title: String,
    notes: Vec<String>,
) -> Result<crate::types::LocalQueueJob, String> {
    crate::local_queue::enqueue_workspace_summary(
        &app,
        transcript_ids,
        template_id,
        workspace_folder,
        workspace_title,
        notes,
    )
    .await
}

#[tauri::command]
pub async fn enqueue_workspace_text_summary(
    app: AppHandle,
    template_id: String,
    workspace_folder: Option<String>,
    workspace_title: String,
    documents: Vec<WorkspaceTextDocument>,
    title: String,
    summary_scope: String,
) -> Result<crate::types::LocalQueueJob, String> {
    crate::local_queue::enqueue_workspace_text_summary(
        &app,
        template_id,
        workspace_folder,
        workspace_title,
        documents,
        title,
        summary_scope,
    )
    .await
}

#[tauri::command]
pub async fn list_local_queue_jobs(
    workspace_folder: Option<String>,
    queue_type: Option<String>,
) -> Result<Vec<crate::types::LocalQueueJob>, String> {
    db::list_local_queue_jobs(workspace_folder.as_deref(), queue_type.as_deref(), None)
}

#[tauri::command]
pub async fn cancel_local_queue_job(
    app: AppHandle,
    queue_type: String,
    job_id: String,
) -> Result<crate::types::LocalQueueJob, String> {
    let job = db::cancel_local_queue_job(&queue_type, &job_id)?;
    crate::local_queue::emit_queue_update(&app, &job);
    Ok(job)
}

#[tauri::command]
pub async fn mark_local_queue_job_synced(
    queue_type: String,
    job_id: String,
) -> Result<crate::types::LocalQueueJob, String> {
    db::mark_local_queue_job_synced(&queue_type, &job_id)
}

#[tauri::command]
pub async fn export_transcript(transcript_id: String, target_path: String) -> Result<(), String> {
    let transcript = db::get_transcript(&transcript_id)?;
    write_text_file(&target_path, &transcript.text)
}

#[tauri::command]
pub async fn export_summary(summary_id: String, target_path: String) -> Result<(), String> {
    let summary = db::get_summary(&summary_id)?;
    write_text_file(&target_path, &summary.content)
}

fn model_status_for_settings(settings: &AppSettings) -> Result<ModelStatus, String> {
    let repo_key = model_status_repo_key(settings);
    let actual_repo = resolved_asr_model_repo(settings);
    if let Some(path) = settings
        .asr_model_path
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        let ready = Path::new(path).exists();
        let auxiliary_ready = ready && auxiliary_models_ready(settings, path)?;
        return Ok(ModelStatus {
            repo: actual_repo,
            status: if ready && auxiliary_ready {
                "ready"
            } else {
                "missing"
            }
            .to_string(),
            path: Some(path.clone()),
            message: if ready && auxiliary_ready {
                Some("Using configured local model path".to_string())
            } else if ready {
                Some("ASR workflow auxiliary models are not downloaded".to_string())
            } else {
                Some("Configured local model path does not exist".to_string())
            },
            updated_at: None,
        });
    }

    let mut status = db::get_model_status(&repo_key)?;
    status.repo = actual_repo;
    if status.status == "downloading" {
        status.message = Some(format!(
            "Downloading FunASR workflow models from {}",
            model_source_label(&settings.asr_model_source)
        ));
    }
    if let Some(path) = status.path.as_ref() {
        if status.status == "ready" && !Path::new(path).exists() {
            status.status = "missing".to_string();
            status.message = Some("Previously downloaded model path is missing".to_string());
            status.repo = repo_key.clone();
            status = db::save_model_status(status)?;
            status.repo = resolved_asr_model_repo(settings);
        } else if status.status == "ready" && !auxiliary_models_ready(settings, path)? {
            status.status = "missing".to_string();
            status.message = Some("ASR workflow auxiliary models are not downloaded".to_string());
            status.repo = repo_key.clone();
            status = db::save_model_status(status)?;
            status.repo = resolved_asr_model_repo(settings);
        }
    }
    Ok(status)
}

fn resolved_asr_model_repo(settings: &AppSettings) -> String {
    let configured = settings.asr_model_repo.trim();
    match settings.asr_model_source.as_str() {
        "modelscope"
            if configured.is_empty()
                || configured == FUNASR_WORKFLOW_DEFAULT_REPO
                || configured == FUNASR_LEGACY_NANO_REPO =>
        {
            FUNASR_WORKFLOW_DEFAULT_REPO.to_string()
        }
        "huggingface" if configured.is_empty() || configured == FUNASR_LEGACY_NANO_REPO => {
            FUNASR_WORKFLOW_DEFAULT_REPO.to_string()
        }
        _ if configured.is_empty() => FUNASR_WORKFLOW_DEFAULT_REPO.to_string(),
        _ => configured.to_string(),
    }
}

fn model_status_repo_key(settings: &AppSettings) -> String {
    format!(
        "{}::{}",
        settings.asr_model_source,
        resolved_asr_model_repo(settings)
    )
}

fn repo_model_target_dir(settings: &AppSettings) -> Result<PathBuf, String> {
    Ok(db::models_dir()?.join(format!(
        "{}__{}",
        settings.asr_model_source,
        resolved_asr_model_repo(settings).replace('/', "__")
    )))
}

fn custom_auxiliary_model_root() -> Result<PathBuf, String> {
    Ok(db::models_dir()?.join(FUNASR_CUSTOM_AUXILIARY_DIR))
}

fn model_download_root_for_settings(settings: &AppSettings) -> Result<PathBuf, String> {
    if settings
        .asr_model_path
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return custom_auxiliary_model_root();
    }
    repo_model_target_dir(settings)
}

async fn estimate_model_download_total_bytes(
    app: &AppHandle,
    settings: &AppSettings,
    repo: &str,
    profile: DownloadSizeProfile,
) -> Option<u64> {
    let request = sidecar::estimate_model_download_size_request(
        &Uuid::new_v4().to_string(),
        repo,
        &settings.asr_model_source,
        profile.as_str(),
    );
    match sidecar::run_sidecar(app, request, Some(settings)).await {
        Ok(response) => response
            .pointer("/result/total_bytes")
            .and_then(|value| value.as_u64())
            .or_else(|| {
                log::warn!(
                    "Model download size estimate missing total_bytes: repo={} source={} profile={} response={}",
                    repo,
                    settings.asr_model_source,
                    profile.as_str(),
                    response
                );
                None
            }),
        Err(error) => {
            log::warn!(
                "Model download size estimate failed: repo={} source={} profile={} error={}",
                repo,
                settings.asr_model_source,
                profile.as_str(),
                error
            );
            None
        }
    }
}

pub(crate) fn auxiliary_model_paths_for_queue(
    settings: &AppSettings,
    model_path: &str,
) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), String> {
    let root = if settings
        .asr_model_path
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        custom_auxiliary_model_root()?
    } else {
        Path::new(model_path).join(FUNASR_AUXILIARY_DIR)
    };
    Ok((
        root.clone(),
        root.join("fsmn-vad"),
        root.join("campplus"),
        root.join("ct-punc-c"),
    ))
}

fn required_model_file_ready(path: &Path, file_name: &str) -> bool {
    path.is_dir() && path.join(file_name).is_file()
}

fn asr_model_ready(path: &Path) -> bool {
    required_model_file_ready(path, "model.pt")
}

fn vad_model_ready(path: &Path) -> bool {
    required_model_file_ready(path, "model.pt")
}

fn speaker_model_ready(path: &Path) -> bool {
    required_model_file_ready(path, "campplus_cn_common.bin")
}

fn punc_model_ready(path: &Path) -> bool {
    required_model_file_ready(path, "model.pt")
}

fn auxiliary_models_ready(settings: &AppSettings, model_path: &str) -> Result<bool, String> {
    let (_root, vad_model_path, speaker_model_path, punc_model_path) =
        auxiliary_model_paths_for_queue(settings, model_path)?;
    Ok(vad_model_ready(&vad_model_path)
        && speaker_model_ready(&speaker_model_path)
        && punc_model_ready(&punc_model_path))
}

fn dictation_models_ready(settings: &AppSettings, model_path: &str) -> Result<bool, String> {
    let (_root, _vad_model_path, _speaker_model_path, punc_model_path) =
        auxiliary_model_paths_for_queue(settings, model_path)?;
    Ok(asr_model_ready(Path::new(model_path)) && punc_model_ready(&punc_model_path))
}

pub(crate) fn dictation_model_paths_if_ready_for_queue(
    settings: &AppSettings,
) -> Result<Option<(String, PathBuf)>, String> {
    if let Some(path) = settings
        .asr_model_path
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        if Path::new(path).exists() && dictation_models_ready(settings, path)? {
            let (_root, _vad_model_path, _speaker_model_path, punc_model_path) =
                auxiliary_model_paths_for_queue(settings, path)?;
            return Ok(Some((path.clone(), punc_model_path)));
        }
        return Ok(None);
    }

    let current = db::get_model_status(&model_status_repo_key(settings))?;
    if let Some(path) = current.path.as_ref() {
        if Path::new(path).exists() && dictation_models_ready(settings, path)? {
            let (_root, _vad_model_path, _speaker_model_path, punc_model_path) =
                auxiliary_model_paths_for_queue(settings, path)?;
            return Ok(Some((path.clone(), punc_model_path)));
        }
    }

    let target_dir = repo_model_target_dir(settings)?;
    let target_dir_string = target_dir.to_string_lossy().to_string();
    if dictation_models_ready(settings, &target_dir_string)? {
        let (_root, _vad_model_path, _speaker_model_path, punc_model_path) =
            auxiliary_model_paths_for_queue(settings, &target_dir_string)?;
        return Ok(Some((target_dir_string, punc_model_path)));
    }

    Ok(None)
}

async fn clear_active_model_download(kind: ActiveDownloadKind) {
    let mut active = ACTIVE_MODEL_DOWNLOAD.lock().await;
    if active
        .as_ref()
        .map(|download| download.kind == kind)
        .unwrap_or(false)
    {
        *active = None;
    }
}

pub(crate) async fn ensure_model_ready_for_queue(
    app: &AppHandle,
    settings: &AppSettings,
) -> Result<ModelStatus, String> {
    let _download_guard = MODEL_DOWNLOAD_LOCK.lock().await;
    let actual_repo = resolved_asr_model_repo(settings);
    let repo_key = model_status_repo_key(settings);
    {
        let mut active = ACTIVE_MODEL_DOWNLOAD.lock().await;
        *active = None;
    }

    if let Some(path) = settings
        .asr_model_path
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        if Path::new(path).exists() {
            let auxiliary_root = custom_auxiliary_model_root()?;
            ensure_auxiliary_models_ready(app, settings, &auxiliary_root).await?;
            return Ok(ModelStatus {
                repo: actual_repo,
                status: "ready".to_string(),
                path: Some(path.clone()),
                message: Some("Using configured local model path".to_string()),
                updated_at: None,
            });
        }
        return Err(format!("Configured ASR model path does not exist: {path}"));
    }

    let current = db::get_model_status(&repo_key)?;
    if current.status == "ready" {
        if let Some(path) = current.path.as_ref() {
            if Path::new(path).exists() && auxiliary_models_ready(settings, path)? {
                return Ok(ModelStatus {
                    repo: actual_repo,
                    ..current
                });
            }
        }
    }

    let _ = db::save_model_status(ModelStatus {
        repo: repo_key.clone(),
        status: "downloading".to_string(),
        path: current.path,
        message: Some(format!(
            "Downloading FunASR workflow models from {}",
            model_source_label(&settings.asr_model_source)
        )),
        updated_at: None,
    })?;

    let target_dir = repo_model_target_dir(settings)?;
    let total_bytes = estimate_model_download_total_bytes(
        app,
        settings,
        &actual_repo,
        DownloadSizeProfile::Pipeline,
    )
    .await;
    let (cancel_send, cancel_recv) = oneshot::channel();
    {
        let mut active = ACTIVE_MODEL_DOWNLOAD.lock().await;
        *active = Some(ActiveModelDownload {
            kind: ActiveDownloadKind::Main,
            repo: actual_repo.clone(),
            source: settings.asr_model_source.clone(),
            target_dir: target_dir.clone(),
            model_path: None,
            total_bytes,
            cancel: Some(cancel_send),
        });
    }
    let (progress_done, progress_stop) = oneshot::channel();
    let progress_app = app.clone();
    let progress_repo = actual_repo.clone();
    let progress_source = settings.asr_model_source.clone();
    let progress_target = target_dir.clone();
    let progress_total_bytes = total_bytes;
    let progress_handle = tokio::spawn(async move {
        watch_model_download_progress(
            progress_app,
            progress_repo,
            progress_source,
            progress_target,
            progress_total_bytes,
            progress_stop,
        )
        .await;
    });

    let request = sidecar::prepare_model_request(
        &Uuid::new_v4().to_string(),
        &actual_repo,
        &target_dir.to_string_lossy(),
        &settings.asr_model_source,
    );

    let response =
        match sidecar::run_sidecar_with_cancel(app, request, Some(settings), Some(cancel_recv))
            .await
        {
            Ok(value) => value,
            Err(error) => {
                clear_active_model_download(ActiveDownloadKind::Main).await;
                let _ = progress_done.send(());
                let _ = progress_handle.await;
                let cancelled = error.starts_with("MODEL_DOWNLOAD_CANCELLED");
                emit_model_download_progress_with_total(
                    app,
                    &actual_repo,
                    &settings.asr_model_source,
                    if cancelled { "cancelled" } else { "failed" },
                    scan_dir_size(&target_dir),
                    total_bytes,
                    Some(if cancelled {
                        "FunASR workflow model download cancelled".to_string()
                    } else {
                        error.clone()
                    }),
                );
                let failed = ModelStatus {
                    repo: repo_key.clone(),
                    status: if cancelled { "cancelled" } else { "failed" }.to_string(),
                    path: None,
                    message: Some(if cancelled {
                        "FunASR workflow model download cancelled".to_string()
                    } else {
                        error.clone()
                    }),
                    updated_at: None,
                };
                let _ = db::save_model_status(failed);
                return Err(error);
            }
        };
    clear_active_model_download(ActiveDownloadKind::Main).await;
    let _ = progress_done.send(());
    let _ = progress_handle.await;

    let model_path = response
        .pointer("/result/model_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "Model download response missing model_path".to_string())?
        .to_string();
    emit_model_download_progress_with_total(
        app,
        &actual_repo,
        &settings.asr_model_source,
        "ready",
        scan_dir_size(Path::new(&model_path)),
        total_bytes,
        Some("FunASR workflow models are ready".to_string()),
    );
    let mut status = db::save_model_status(ModelStatus {
        repo: repo_key,
        status: "ready".to_string(),
        path: Some(model_path),
        message: Some("FunASR workflow models are ready".to_string()),
        updated_at: None,
    })?;
    status.repo = actual_repo;
    Ok(status)
}

pub(crate) async fn ensure_dictation_model_ready_for_queue(
    app: &AppHandle,
    settings: &AppSettings,
) -> Result<ModelStatus, String> {
    let _download_guard = MODEL_DOWNLOAD_LOCK.lock().await;
    let actual_repo = resolved_asr_model_repo(settings);
    let repo_key = model_status_repo_key(settings);
    {
        let mut active = ACTIVE_MODEL_DOWNLOAD.lock().await;
        *active = None;
    }

    if let Some(path) = settings
        .asr_model_path
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        if Path::new(path).exists() {
            let auxiliary_root = custom_auxiliary_model_root()?;
            ensure_punctuation_model_ready(app, settings, &auxiliary_root).await?;
            return Ok(ModelStatus {
                repo: actual_repo,
                status: "ready".to_string(),
                path: Some(path.clone()),
                message: Some("Using configured local model path for dictation".to_string()),
                updated_at: None,
            });
        }
        return Err(format!("Configured ASR model path does not exist: {path}"));
    }

    let current = db::get_model_status(&repo_key)?;
    if let Some(path) = current.path.as_ref() {
        if Path::new(path).exists() && dictation_models_ready(settings, path)? {
            let path = path.clone();
            return Ok(ModelStatus {
                repo: actual_repo,
                status: "ready".to_string(),
                path: Some(path),
                message: Some("FunASR dictation models are ready".to_string()),
                updated_at: current.updated_at.clone(),
            });
        }
    }

    let target_dir = repo_model_target_dir(settings)?;
    let target_dir_string = target_dir.to_string_lossy().to_string();
    if dictation_models_ready(settings, &target_dir_string)? {
        let mut status = db::save_model_status(ModelStatus {
            repo: repo_key,
            status: "ready".to_string(),
            path: Some(target_dir_string),
            message: Some("FunASR dictation models are ready".to_string()),
            updated_at: None,
        })?;
        status.repo = actual_repo;
        return Ok(status);
    }

    let _ = db::save_model_status(ModelStatus {
        repo: repo_key.clone(),
        status: "downloading".to_string(),
        path: current.path,
        message: Some(format!(
            "Downloading FunASR dictation models from {}",
            model_source_label(&settings.asr_model_source)
        )),
        updated_at: None,
    })?;

    let total_bytes = estimate_model_download_total_bytes(
        app,
        settings,
        &actual_repo,
        DownloadSizeProfile::Dictation,
    )
    .await;
    let (cancel_send, cancel_recv) = oneshot::channel();
    {
        let mut active = ACTIVE_MODEL_DOWNLOAD.lock().await;
        *active = Some(ActiveModelDownload {
            kind: ActiveDownloadKind::Main,
            repo: actual_repo.clone(),
            source: settings.asr_model_source.clone(),
            target_dir: target_dir.clone(),
            model_path: None,
            total_bytes,
            cancel: Some(cancel_send),
        });
    }
    let (progress_done, progress_stop) = oneshot::channel();
    let progress_app = app.clone();
    let progress_repo = actual_repo.clone();
    let progress_source = settings.asr_model_source.clone();
    let progress_target = target_dir.clone();
    let progress_total_bytes = total_bytes;
    let progress_handle = tokio::spawn(async move {
        watch_model_download_progress(
            progress_app,
            progress_repo,
            progress_source,
            progress_target,
            progress_total_bytes,
            progress_stop,
        )
        .await;
    });

    let request = sidecar::prepare_dictation_models_request(
        &Uuid::new_v4().to_string(),
        &actual_repo,
        &target_dir.to_string_lossy(),
        &settings.asr_model_source,
    );

    let response =
        match sidecar::run_sidecar_with_cancel(app, request, Some(settings), Some(cancel_recv))
            .await
        {
            Ok(value) => value,
            Err(error) => {
                clear_active_model_download(ActiveDownloadKind::Main).await;
                let _ = progress_done.send(());
                let _ = progress_handle.await;
                let cancelled = error.starts_with("MODEL_DOWNLOAD_CANCELLED");
                emit_model_download_progress_with_total(
                    app,
                    &actual_repo,
                    &settings.asr_model_source,
                    if cancelled { "cancelled" } else { "failed" },
                    scan_dir_size(&target_dir),
                    total_bytes,
                    Some(if cancelled {
                        "FunASR dictation model download cancelled".to_string()
                    } else {
                        error.clone()
                    }),
                );
                let failed = ModelStatus {
                    repo: repo_key.clone(),
                    status: if cancelled { "cancelled" } else { "failed" }.to_string(),
                    path: None,
                    message: Some(if cancelled {
                        "FunASR dictation model download cancelled".to_string()
                    } else {
                        error.clone()
                    }),
                    updated_at: None,
                };
                let _ = db::save_model_status(failed);
                return Err(error);
            }
        };
    clear_active_model_download(ActiveDownloadKind::Main).await;
    let _ = progress_done.send(());
    let _ = progress_handle.await;

    let model_path = response
        .pointer("/result/model_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "Dictation model download response missing model_path".to_string())?
        .to_string();
    if !dictation_models_ready(settings, &model_path)? {
        return Err(
            "FunASR dictation model download finished without ready model paths".to_string(),
        );
    }

    emit_model_download_progress_with_total(
        app,
        &actual_repo,
        &settings.asr_model_source,
        "ready",
        scan_dir_size(Path::new(&model_path)),
        total_bytes,
        Some("FunASR dictation models are ready".to_string()),
    );
    let mut status = db::save_model_status(ModelStatus {
        repo: repo_key,
        status: "ready".to_string(),
        path: Some(model_path),
        message: Some("FunASR dictation models are ready".to_string()),
        updated_at: None,
    })?;
    status.repo = actual_repo;
    Ok(status)
}

async fn ensure_auxiliary_models_ready(
    app: &AppHandle,
    settings: &AppSettings,
    auxiliary_root: &Path,
) -> Result<(), String> {
    let actual_repo = resolved_asr_model_repo(settings);
    let vad_model_path = auxiliary_root.join("fsmn-vad");
    let speaker_model_path = auxiliary_root.join("campplus");
    let punc_model_path = auxiliary_root.join("ct-punc-c");
    if vad_model_ready(&vad_model_path)
        && speaker_model_ready(&speaker_model_path)
        && punc_model_ready(&punc_model_path)
    {
        return Ok(());
    }

    let total_bytes = estimate_model_download_total_bytes(
        app,
        settings,
        &actual_repo,
        DownloadSizeProfile::Auxiliary,
    )
    .await;
    let (cancel_send, cancel_recv) = oneshot::channel();
    {
        let mut active = ACTIVE_MODEL_DOWNLOAD.lock().await;
        *active = Some(ActiveModelDownload {
            kind: ActiveDownloadKind::Auxiliary,
            repo: actual_repo.clone(),
            source: settings.asr_model_source.clone(),
            target_dir: auxiliary_root.to_path_buf(),
            model_path: settings.asr_model_path.as_ref().map(PathBuf::from),
            total_bytes,
            cancel: Some(cancel_send),
        });
    }
    let (progress_done, progress_stop) = oneshot::channel();
    let progress_app = app.clone();
    let progress_repo = actual_repo.clone();
    let progress_source = settings.asr_model_source.clone();
    let progress_target = auxiliary_root.to_path_buf();
    let progress_total_bytes = total_bytes;
    let progress_handle = tokio::spawn(async move {
        watch_model_download_progress(
            progress_app,
            progress_repo,
            progress_source,
            progress_target,
            progress_total_bytes,
            progress_stop,
        )
        .await;
    });

    let request = sidecar::prepare_auxiliary_models_request(
        &Uuid::new_v4().to_string(),
        &auxiliary_root.to_string_lossy(),
        &settings.asr_model_source,
    );
    let response =
        match sidecar::run_sidecar_with_cancel(app, request, Some(settings), Some(cancel_recv))
            .await
        {
            Ok(value) => value,
            Err(error) => {
                clear_active_model_download(ActiveDownloadKind::Auxiliary).await;
                let _ = progress_done.send(());
                let _ = progress_handle.await;
                let cancelled = error.starts_with("MODEL_DOWNLOAD_CANCELLED");
                emit_model_download_progress_with_total(
                    app,
                    &actual_repo,
                    &settings.asr_model_source,
                    if cancelled { "cancelled" } else { "failed" },
                    scan_dir_size(auxiliary_root),
                    total_bytes,
                    Some(if cancelled {
                        "FunASR workflow model download cancelled".to_string()
                    } else {
                        error.clone()
                    }),
                );
                return Err(error);
            }
        };
    clear_active_model_download(ActiveDownloadKind::Auxiliary).await;
    let _ = progress_done.send(());
    let _ = progress_handle.await;

    let vad_ready = response
        .pointer("/result/vad_model_path")
        .and_then(|value| value.as_str())
        .map(|path| vad_model_ready(Path::new(path)))
        .unwrap_or(false);
    let speaker_ready = response
        .pointer("/result/speaker_model_path")
        .and_then(|value| value.as_str())
        .map(|path| speaker_model_ready(Path::new(path)))
        .unwrap_or(false);
    let punc_ready = response
        .pointer("/result/punc_model_path")
        .and_then(|value| value.as_str())
        .map(|path| punc_model_ready(Path::new(path)))
        .unwrap_or(false);
    if !vad_ready || !speaker_ready || !punc_ready {
        return Err("ASR workflow model download finished without ready model paths".to_string());
    }

    emit_model_download_progress_with_total(
        app,
        &actual_repo,
        &settings.asr_model_source,
        "ready",
        scan_dir_size(auxiliary_root),
        total_bytes,
        Some("ASR workflow auxiliary models are ready".to_string()),
    );
    Ok(())
}

async fn ensure_punctuation_model_ready(
    app: &AppHandle,
    settings: &AppSettings,
    auxiliary_root: &Path,
) -> Result<(), String> {
    let actual_repo = resolved_asr_model_repo(settings);
    let punc_model_path = auxiliary_root.join("ct-punc-c");
    if punc_model_ready(&punc_model_path) {
        return Ok(());
    }

    let total_bytes = estimate_model_download_total_bytes(
        app,
        settings,
        &actual_repo,
        DownloadSizeProfile::Punctuation,
    )
    .await;
    let (cancel_send, cancel_recv) = oneshot::channel();
    {
        let mut active = ACTIVE_MODEL_DOWNLOAD.lock().await;
        *active = Some(ActiveModelDownload {
            kind: ActiveDownloadKind::Punctuation,
            repo: actual_repo.clone(),
            source: settings.asr_model_source.clone(),
            target_dir: auxiliary_root.to_path_buf(),
            model_path: settings.asr_model_path.as_ref().map(PathBuf::from),
            total_bytes,
            cancel: Some(cancel_send),
        });
    }
    let (progress_done, progress_stop) = oneshot::channel();
    let progress_app = app.clone();
    let progress_repo = actual_repo.clone();
    let progress_source = settings.asr_model_source.clone();
    let progress_target = auxiliary_root.to_path_buf();
    let progress_total_bytes = total_bytes;
    let progress_handle = tokio::spawn(async move {
        watch_model_download_progress(
            progress_app,
            progress_repo,
            progress_source,
            progress_target,
            progress_total_bytes,
            progress_stop,
        )
        .await;
    });

    let request = sidecar::prepare_punctuation_model_request(
        &Uuid::new_v4().to_string(),
        &auxiliary_root.to_string_lossy(),
        &settings.asr_model_source,
    );
    let response =
        match sidecar::run_sidecar_with_cancel(app, request, Some(settings), Some(cancel_recv))
            .await
        {
            Ok(value) => value,
            Err(error) => {
                clear_active_model_download(ActiveDownloadKind::Punctuation).await;
                let _ = progress_done.send(());
                let _ = progress_handle.await;
                let cancelled = error.starts_with("MODEL_DOWNLOAD_CANCELLED");
                emit_model_download_progress_with_total(
                    app,
                    &actual_repo,
                    &settings.asr_model_source,
                    if cancelled { "cancelled" } else { "failed" },
                    scan_dir_size(auxiliary_root),
                    total_bytes,
                    Some(if cancelled {
                        "FunASR punctuation model download cancelled".to_string()
                    } else {
                        error.clone()
                    }),
                );
                return Err(error);
            }
        };
    clear_active_model_download(ActiveDownloadKind::Punctuation).await;
    let _ = progress_done.send(());
    let _ = progress_handle.await;

    let punc_ready = response
        .pointer("/result/punc_model_path")
        .and_then(|value| value.as_str())
        .map(|path| punc_model_ready(Path::new(path)))
        .unwrap_or(false);
    if !punc_ready {
        return Err(
            "FunASR punctuation model download finished without ready model path".to_string(),
        );
    }

    emit_model_download_progress_with_total(
        app,
        &actual_repo,
        &settings.asr_model_source,
        "ready",
        scan_dir_size(auxiliary_root),
        total_bytes,
        Some("FunASR punctuation model is ready".to_string()),
    );
    Ok(())
}

async fn watch_model_download_progress(
    app: AppHandle,
    repo: String,
    source: String,
    target_dir: std::path::PathBuf,
    total_bytes: Option<u64>,
    mut stop: oneshot::Receiver<()>,
) {
    let started = Instant::now();
    let mut samples: VecDeque<(Instant, u64)> = VecDeque::new();
    let initial_bytes = scan_dir_size(&target_dir);
    let mut last_growth_at = Instant::now();
    let mut last_seen_bytes = initial_bytes;
    samples.push_back((Instant::now(), initial_bytes));
    emit_model_download_progress_with_total(
        &app,
        &repo,
        &source,
        "downloading",
        initial_bytes,
        total_bytes,
        Some(format!("正在连接 {} 下载源", model_source_label(&source))),
    );
    log::info!(
        "ASR model download started: repo={} source={} bytes={}",
        repo,
        source,
        initial_bytes
    );
    let mut last_log_at = Instant::now();

    loop {
        tokio::select! {
            _ = &mut stop => break,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                let now = Instant::now();
                let bytes = scan_dir_size(&target_dir);
                if bytes > last_seen_bytes {
                    last_growth_at = now;
                    last_seen_bytes = bytes;
                }
                samples.push_back((now, bytes));
                while samples
                    .front()
                    .map(|(at, _)| now.duration_since(*at) > Duration::from_secs(8))
                    .unwrap_or(false)
                {
                    samples.pop_front();
                }
                let speed = smoothed_download_speed(&samples);
                let display_speed = if speed <= 0.0
                    && now.duration_since(last_growth_at) <= Duration::from_secs(10)
                {
                    recent_nonzero_speed(&samples)
                } else {
                    speed
                };
                let idle_for = now.duration_since(last_growth_at).as_secs();
                let message = if idle_for >= DOWNLOAD_STALL_MESSAGE_SECS && display_speed <= 0.0 {
                    Some(format!(
                        "{} 秒未收到新数据，请检查下载源或代理连接",
                        idle_for
                    ))
                } else {
                    Some(format!(
                        "正在从 {} 下载，已运行 {} 秒",
                        model_source_label(&source),
                        started.elapsed().as_secs()
                    ))
                };

                emit_model_download_progress_with_total_and_speed(
                    &app,
                    &repo,
                    &source,
                    "downloading",
                    bytes,
                    total_bytes,
                    display_speed,
                    message,
                );
                if now.duration_since(last_log_at) >= Duration::from_secs(5) {
                    log::info!(
                        "ASR model download progress: repo={} source={} bytes={} speed_bps={:.0} idle_secs={}",
                        repo,
                        source,
                        bytes,
                        display_speed,
                        idle_for
                    );
                    last_log_at = now;
                }
            }
        }
    }
}

fn smoothed_download_speed(samples: &VecDeque<(Instant, u64)>) -> f64 {
    let Some((first_at, first_bytes)) = samples.front() else {
        return 0.0;
    };
    let Some((last_at, last_bytes)) = samples.back() else {
        return 0.0;
    };
    let elapsed = last_at.duration_since(*first_at).as_secs_f64();
    if elapsed <= 0.0 || last_bytes <= first_bytes {
        return 0.0;
    }
    (last_bytes - first_bytes) as f64 / elapsed
}

fn recent_nonzero_speed(samples: &VecDeque<(Instant, u64)>) -> f64 {
    let mut previous: Option<(Instant, u64)> = None;
    for sample in samples.iter().rev() {
        if let Some((prev_at, prev_bytes)) = previous {
            if prev_bytes > sample.1 {
                let elapsed = prev_at.duration_since(sample.0).as_secs_f64().max(0.001);
                return (prev_bytes - sample.1) as f64 / elapsed;
            }
        }
        previous = Some(*sample);
    }
    0.0
}

fn emit_model_download_progress(
    app: &AppHandle,
    repo: &str,
    source: &str,
    status: &str,
    downloaded_bytes: u64,
    message: Option<String>,
) {
    emit_model_download_progress_with_speed(
        app,
        repo,
        source,
        status,
        downloaded_bytes,
        0.0,
        message,
    );
}

fn emit_model_download_progress_with_total(
    app: &AppHandle,
    repo: &str,
    source: &str,
    status: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    message: Option<String>,
) {
    emit_model_download_progress_with_total_and_speed(
        app,
        repo,
        source,
        status,
        downloaded_bytes,
        total_bytes,
        0.0,
        message,
    );
}

fn emit_model_download_progress_with_speed(
    app: &AppHandle,
    repo: &str,
    source: &str,
    status: &str,
    downloaded_bytes: u64,
    speed_bytes_per_second: f64,
    message: Option<String>,
) {
    let payload = model_download_progress_snapshot(
        repo,
        source,
        status,
        downloaded_bytes,
        speed_bytes_per_second,
        message,
    );
    remember_model_download_progress(&payload);
    let _ = app.emit("asr-model-download-progress", payload);
}

fn emit_model_download_progress_with_total_and_speed(
    app: &AppHandle,
    repo: &str,
    source: &str,
    status: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    speed_bytes_per_second: f64,
    message: Option<String>,
) {
    let payload = model_download_progress_snapshot_with_total(
        repo,
        source,
        status,
        downloaded_bytes,
        speed_bytes_per_second,
        message,
        total_bytes,
    );
    remember_model_download_progress(&payload);
    let _ = app.emit("asr-model-download-progress", payload);
}

fn model_download_progress_snapshot(
    repo: &str,
    source: &str,
    status: &str,
    downloaded_bytes: u64,
    speed_bytes_per_second: f64,
    message: Option<String>,
) -> ModelDownloadProgress {
    model_download_progress_snapshot_with_total(
        repo,
        source,
        status,
        downloaded_bytes,
        speed_bytes_per_second,
        message,
        None,
    )
}

fn model_download_progress_snapshot_with_total(
    repo: &str,
    source: &str,
    status: &str,
    downloaded_bytes: u64,
    speed_bytes_per_second: f64,
    message: Option<String>,
    total_bytes: Option<u64>,
) -> ModelDownloadProgress {
    let percent = total_bytes.map(|total| {
        if status == "ready" {
            100.0
        } else if total == 0 {
            0.0
        } else {
            ((downloaded_bytes as f64 / total as f64) * 100.0).min(99.9)
        }
    });
    ModelDownloadProgress {
        repo: repo.to_string(),
        source: source.to_string(),
        status: status.to_string(),
        downloaded_bytes,
        total_bytes,
        speed_bytes_per_second,
        percent,
        message,
    }
}

fn remember_model_download_progress(progress: &ModelDownloadProgress) {
    let now = Instant::now();
    let Ok(mut cache) = MODEL_DOWNLOAD_CACHE.lock() else {
        return;
    };
    cache.snapshot = Some(progress.clone());
    cache.last_sample = Some((now, progress.downloaded_bytes));
    if progress.speed_bytes_per_second > 0.0 {
        cache.last_nonzero_speed = progress.speed_bytes_per_second;
        cache.last_nonzero_at = Some(now);
        cache.last_growth_at = Some(now);
    }
}

fn cached_model_download_progress(
    repo: &str,
    source: &str,
    current_status: &str,
    downloaded_bytes: u64,
    current_message: Option<String>,
) -> ModelDownloadProgress {
    let now = Instant::now();
    let Ok(mut cache) = MODEL_DOWNLOAD_CACHE.lock() else {
        return model_download_progress_snapshot(
            repo,
            source,
            current_status,
            downloaded_bytes,
            0.0,
            current_message,
        );
    };

    let cached = cache
        .snapshot
        .as_ref()
        .filter(|progress| progress.repo == repo && progress.source == source)
        .cloned();
    let mut speed = cached
        .as_ref()
        .map(|progress| progress.speed_bytes_per_second)
        .unwrap_or(0.0);
    if let Some((last_at, last_bytes)) = cache.last_sample {
        if downloaded_bytes > last_bytes {
            let elapsed = now.duration_since(last_at).as_secs_f64().max(0.001);
            speed = (downloaded_bytes - last_bytes) as f64 / elapsed;
            cache.last_nonzero_speed = speed;
            cache.last_nonzero_at = Some(now);
            cache.last_growth_at = Some(now);
        } else if cache
            .last_nonzero_at
            .map(|at| now.duration_since(at) > Duration::from_secs(DOWNLOAD_SPEED_HOLD_SECS))
            .unwrap_or(true)
        {
            speed = 0.0;
        } else {
            speed = cache.last_nonzero_speed;
        }
    }

    let status = cached
        .as_ref()
        .map(|progress| progress.status.as_str())
        .unwrap_or(current_status);
    let mut message = cached
        .as_ref()
        .and_then(|progress| progress.message.clone())
        .or(current_message);
    if status == "downloading" && speed <= 0.0 {
        if let Some(last_growth_at) = cache.last_growth_at {
            let idle_for = now.duration_since(last_growth_at).as_secs();
            if idle_for >= DOWNLOAD_STALL_MESSAGE_SECS {
                message = Some(format!(
                    "{} 秒未收到新数据，请检查下载源或代理连接",
                    idle_for
                ));
            }
        }
    }
    cache.last_sample = Some((now, downloaded_bytes));

    let total_bytes = cached.as_ref().and_then(|progress| progress.total_bytes);
    let progress = model_download_progress_snapshot_with_total(
        repo,
        source,
        status,
        downloaded_bytes,
        speed,
        message,
        total_bytes,
    );
    cache.snapshot = Some(progress.clone());
    progress
}

fn scan_dir_size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&current) else {
            continue;
        };
        if metadata.is_file() {
            total += metadata.len();
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            stack.push(entry.path());
        }
    }
    total
}

fn model_source_label(source: &str) -> &str {
    match source {
        "modelscope" => "ModelScope",
        "huggingface" => "Hugging Face",
        _ => source,
    }
}

pub(crate) fn write_transcript_files_for_queue(
    transcript: &TranscriptRecord,
) -> Result<(), String> {
    let dir = db::transcripts_dir()?;
    std::fs::write(
        dir.join(format!("{}.txt", transcript.id)),
        transcript.text.as_bytes(),
    )
    .map_err(|e| format!("Failed to write transcript text: {e}"))?;
    std::fs::write(
        dir.join(format!("{}.json", transcript.id)),
        transcript.result_json.as_bytes(),
    )
    .map_err(|e| format!("Failed to write transcript JSON: {e}"))?;
    Ok(())
}

pub(crate) fn write_summary_files_for_queue(summary: &SummaryRecord) -> Result<(), String> {
    let dir = db::summaries_dir()?;
    std::fs::write(
        dir.join(format!("{}.md", summary.id)),
        summary.content.as_bytes(),
    )
    .map_err(|e| format!("Failed to write summary Markdown: {e}"))?;
    let json = json!({
        "id": summary.id,
        "transcript_id": summary.transcript_id,
        "template_id": summary.template_id,
        "provider": summary.provider,
        "model": summary.model,
        "content": summary.content,
        "result_json": summary.result_json,
        "created_at": summary.created_at
    });
    std::fs::write(
        dir.join(format!("{}.json", summary.id)),
        json.to_string().as_bytes(),
    )
    .map_err(|e| format!("Failed to write summary JSON: {e}"))?;
    Ok(())
}

fn write_text_file(path: &str, content: &str) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create export directory: {e}"))?;
    }
    std::fs::write(path, content.as_bytes()).map_err(|e| format!("Failed to write export: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> AppSettings {
        AppSettings {
            python_path: String::new(),
            ffmpeg_path: String::new(),
            asr_model_repo: FUNASR_WORKFLOW_DEFAULT_REPO.to_string(),
            asr_model_source: "modelscope".to_string(),
            asr_model_path: None,
            http_proxy: None,
            https_proxy: None,
            all_proxy: None,
            use_gpu: true,
            llm_provider: String::new(),
            llm_base_url: String::new(),
            llm_model: String::new(),
            llm_temperature: 0.0,
            llm_max_tokens: 0,
            llm_timeout_seconds: 0,
            has_llm_api_key: false,
            voice_input_enabled: true,
            voice_input_hotkey: String::new(),
            voice_input_refinement_mode: "local".to_string(),
            voice_input_refinement_prompt: String::new(),
        }
    }

    #[test]
    fn progress_snapshot_uses_unknown_total_when_no_dynamic_total_is_available() {
        let progress = model_download_progress_snapshot(
            FUNASR_WORKFLOW_DEFAULT_REPO,
            "modelscope",
            "downloading",
            100,
            0.0,
            None,
        );

        assert_eq!(progress.total_bytes, None);
        assert_eq!(progress.percent, None);
    }

    #[test]
    fn progress_snapshot_uses_dynamic_total_when_available() {
        let downloaded_bytes = 512 * 1024;
        let total_bytes = 1024 * 1024;
        let progress = model_download_progress_snapshot_with_total(
            FUNASR_WORKFLOW_DEFAULT_REPO,
            "modelscope",
            "downloading",
            downloaded_bytes,
            0.0,
            None,
            Some(total_bytes),
        );

        assert_eq!(progress.downloaded_bytes, downloaded_bytes);
        assert_eq!(progress.total_bytes, Some(total_bytes));
        assert_eq!(progress.percent, Some(50.0));
    }

    #[test]
    fn dictation_model_ready_requires_asr_and_punctuation_weights() {
        let root = std::env::temp_dir().join(format!(
            "voice-vibe-dictation-model-ready-{}",
            Uuid::new_v4()
        ));
        let model_path = root.join("asr");
        let punc_path = model_path.join(FUNASR_AUXILIARY_DIR).join("ct-punc-c");
        std::fs::create_dir_all(&punc_path).expect("create punc path");
        std::fs::write(punc_path.join("config.yaml"), "punc").expect("write punc marker");

        let settings = test_settings();
        let model_path_string = model_path.to_string_lossy().to_string();

        assert!(!dictation_models_ready(&settings, &model_path_string).unwrap());
        std::fs::write(model_path.join("model.pt"), "asr").expect("write asr weight");
        assert!(!dictation_models_ready(&settings, &model_path_string).unwrap());
        std::fs::write(punc_path.join("model.pt"), "punc").expect("write punc weight");

        assert!(dictation_models_ready(&settings, &model_path_string).unwrap());
        assert!(!auxiliary_models_ready(&settings, &model_path_string).unwrap());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn auxiliary_model_ready_requires_required_weights() {
        let root = std::env::temp_dir().join(format!(
            "voice-vibe-auxiliary-model-ready-{}",
            Uuid::new_v4()
        ));
        let model_path = root.join("asr");
        let aux_root = model_path.join(FUNASR_AUXILIARY_DIR);
        let vad_path = aux_root.join("fsmn-vad");
        let speaker_path = aux_root.join("campplus");
        let punc_path = aux_root.join("ct-punc-c");
        std::fs::create_dir_all(&vad_path).expect("create vad path");
        std::fs::create_dir_all(&speaker_path).expect("create speaker path");
        std::fs::create_dir_all(&punc_path).expect("create punc path");
        std::fs::write(vad_path.join("config.yaml"), "vad").expect("write vad marker");
        std::fs::write(speaker_path.join("config.yaml"), "speaker").expect("write speaker marker");
        std::fs::write(punc_path.join("config.yaml"), "punc").expect("write punc marker");

        let settings = test_settings();
        let model_path_string = model_path.to_string_lossy().to_string();

        assert!(!auxiliary_models_ready(&settings, &model_path_string).unwrap());
        std::fs::write(vad_path.join("model.pt"), "vad").expect("write vad weight");
        std::fs::write(speaker_path.join("campplus_cn_common.bin"), "speaker")
            .expect("write speaker weight");
        std::fs::write(punc_path.join("model.pt"), "punc").expect("write punc weight");

        assert!(auxiliary_models_ready(&settings, &model_path_string).unwrap());

        let _ = std::fs::remove_dir_all(root);
    }
}
