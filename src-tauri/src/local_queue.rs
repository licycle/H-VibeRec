use lazy_static::lazy_static;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::asr_worker;
use crate::db;
use crate::llm;
use crate::sidecar;
use crate::types::{
    LocalQueueJob, SummaryJobResult, SummaryRecord, TranscriptRecord, TranscriptionJobResult,
    WorkspaceSummaryResult, WorkspaceTextDocument,
};

lazy_static! {
    static ref TRANSCRIPTION_NOTIFY: Notify = Notify::new();
    static ref SUMMARY_NOTIFY: Notify = Notify::new();
}

pub fn init(app: AppHandle) {
    if let Err(error) = db::recover_interrupted_queue_jobs() {
        log::error!("Failed to recover interrupted local queue jobs: {error}");
    }
    let transcription_app = app.clone();
    tauri::async_runtime::spawn(async move {
        transcription_loop(transcription_app).await;
    });
    let summary_app = app.clone();
    tauri::async_runtime::spawn(async move {
        summary_loop(summary_app).await;
    });
}

pub fn notify_transcription() {
    TRANSCRIPTION_NOTIFY.notify_one();
}

pub fn notify_summary() {
    SUMMARY_NOTIFY.notify_one();
}

pub fn emit_queue_update(app: &AppHandle, job: &LocalQueueJob) {
    let _ = app.emit("local-queue-updated", job);
}

async fn transcription_loop(app: AppHandle) {
    loop {
        match db::pending_transcription_jobs() {
            Ok(jobs) if !jobs.is_empty() => {
                let mut ordered = jobs;
                ordered.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                for job in ordered {
                    match db::get_local_queue_job("transcription", &job.id) {
                        Ok(current) if current.status == "pending" => {
                            if let Err(error) = run_transcription_job(&app, current.clone()).await {
                                let _ = db::fail_transcription_job(
                                    &current.id,
                                    "TRANSCRIPTION_QUEUE_FAILED",
                                    &error,
                                );
                                if let Ok(updated) =
                                    db::get_local_queue_job("transcription", &current.id)
                                {
                                    emit_queue_update(&app, &updated);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => TRANSCRIPTION_NOTIFY.notified().await,
            Err(error) => {
                log::error!("Failed to read transcription queue: {error}");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
}

async fn summary_loop(app: AppHandle) {
    loop {
        match db::pending_summary_jobs() {
            Ok(jobs) if !jobs.is_empty() => {
                let mut ordered = jobs;
                ordered.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                for job in ordered {
                    match db::get_local_queue_job("summary", &job.id) {
                        Ok(current) if current.status == "pending" => {
                            if let Err(error) = run_summary_job(&app, current.clone()).await {
                                let _ = db::fail_summary_job(
                                    &current.id,
                                    "SUMMARY_QUEUE_FAILED",
                                    &error,
                                );
                                if let Ok(updated) = db::get_local_queue_job("summary", &current.id)
                                {
                                    emit_queue_update(&app, &updated);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => SUMMARY_NOTIFY.notified().await,
            Err(error) => {
                log::error!("Failed to read summary queue: {error}");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
}

pub async fn run_transcription_job(
    app: &AppHandle,
    job: LocalQueueJob,
) -> Result<TranscriptionJobResult, String> {
    db::reset_local_queue_pipeline("transcription", &job.id)?;
    db::mark_transcription_job_running(&job.id, 5)?;
    emit_current(app, "transcription", &job.id);

    let result = match run_transcription_job_once(app, &job).await {
        Ok(result) => Ok(result),
        Err(first_error) if should_retry_asr_error(&first_error) && job.retry_count < 1 => {
            db::retry_transcription_job(&job.id, &first_error)?;
            emit_current(app, "transcription", &job.id);
            run_transcription_job_once(app, &job).await
        }
        Err(error) => Err(error),
    };

    match result {
        Ok(result) => {
            db::finish_transcription_job(&job.id)?;
            emit_current(app, "transcription", &job.id);
            enqueue_followup_summary(app, &job, &result.transcript).await;
            Ok(result)
        }
        Err(error) => {
            let _ = db::fail_transcription_job(&job.id, "ASR_FAILED", &error);
            emit_current(app, "transcription", &job.id);
            Err(error)
        }
    }
}

async fn run_transcription_job_once(
    app: &AppHandle,
    job: &LocalQueueJob,
) -> Result<TranscriptionJobResult, String> {
    db::start_local_queue_pipeline_step("transcription", &job.id, "prepare_audio_environment", 10)?;
    let recording_id = job
        .recording_id
        .as_ref()
        .ok_or_else(|| "Transcription job missing recording_id".to_string())?;
    let settings = db::get_settings()?;
    let recording = db::get_recording(recording_id)?;
    db::update_transcription_job_progress(&job.id, 10)?;
    emit_current(app, "transcription", &job.id);

    let model = super::commands::local::ensure_model_ready_for_queue(app, &settings).await?;
    let model_path = model
        .path
        .clone()
        .ok_or_else(|| "ASR model is not ready".to_string())?;
    let (_auxiliary_root, vad_model_path, speaker_model_path, punc_model_path) =
        super::commands::local::auxiliary_model_paths_for_queue(&settings, &model_path)?;
    if !vad_model_path.exists() || !speaker_model_path.exists() || !punc_model_path.exists() {
        return Err(
            "ASR workflow models are not ready; use 下载/检查 FunASR workflow first".to_string(),
        );
    }

    db::update_transcription_job_progress(&job.id, 20)?;
    emit_current(app, "transcription", &job.id);

    let runtime = sidecar::resolve_asr_runtime(app)?;
    let normalized_path = db::normalized_audio_dir()?.join(format!("{}.wav", recording.id));
    let request = sidecar::transcribe_request(
        &job.id,
        &recording.original_audio_path,
        &normalized_path.to_string_lossy(),
        &model_path,
        &runtime.ffmpeg_path.to_string_lossy(),
        settings.use_gpu,
        &vad_model_path.to_string_lossy(),
        &speaker_model_path.to_string_lossy(),
        &punc_model_path.to_string_lossy(),
    );
    let mut request = request;
    if let Some(payload) = request
        .get_mut("payload")
        .and_then(|value| value.as_object_mut())
    {
        payload.insert("reuse_model".to_string(), Value::Bool(true));
    }

    db::update_transcription_job_progress(&job.id, 35)?;
    emit_current(app, "transcription", &job.id);
    db::complete_local_queue_pipeline_step("transcription", &job.id, "prepare_audio_environment")?;
    db::start_local_queue_pipeline_step("transcription", &job.id, "run_asr", 5)?;
    emit_current(app, "transcription", &job.id);
    let response = asr_worker::transcribe(app, request, &settings).await?;

    db::update_transcription_job_progress(&job.id, 90)?;
    db::complete_local_queue_pipeline_step("transcription", &job.id, "run_asr")?;
    db::start_local_queue_pipeline_step("transcription", &job.id, "save_result", 10)?;
    emit_current(app, "transcription", &job.id);
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
        return Err("ASR returned empty transcript".to_string());
    }

    let duration_ms = result
        .get("duration_seconds")
        .and_then(|value| value.as_f64())
        .map(|value| (value * 1000.0) as i64);
    let transcript = db::insert_transcript(
        &recording.id,
        &job.id,
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
    db::attach_transcription_output(&job.id, &transcript.id)?;
    db::complete_local_queue_pipeline_step("transcription", &job.id, "save_result")?;
    db::update_transcription_job_progress(&job.id, 95)?;
    db::start_local_queue_pipeline_step("transcription", &job.id, "write_files", 20)?;
    emit_current(app, "transcription", &job.id);
    super::commands::local::write_transcript_files_for_queue(&transcript)?;
    db::complete_local_queue_pipeline_step("transcription", &job.id, "write_files")?;
    emit_current(app, "transcription", &job.id);
    Ok(TranscriptionJobResult {
        job_id: job.id.clone(),
        transcript,
    })
}

pub async fn run_summary_job(
    app: &AppHandle,
    job: LocalQueueJob,
) -> Result<SummaryJobResult, String> {
    db::reset_local_queue_pipeline("summary", &job.id)?;
    db::mark_summary_job_running(&job.id, 10)?;
    emit_current(app, "summary", &job.id);
    let result = match job.summary_scope.as_deref() {
        Some("workspace") => run_workspace_summary_job(app, &job).await,
        Some("workspace_text") | Some("single_text") => run_text_summary_job(app, &job).await,
        _ => run_single_summary_job(app, &job).await,
    };

    match result {
        Ok(result) => {
            db::finish_summary_job(&job.id)?;
            emit_current(app, "summary", &job.id);
            Ok(result)
        }
        Err(error) => {
            let _ = db::fail_summary_job(&job.id, "LLM_FAILED", &error);
            emit_current(app, "summary", &job.id);
            Err(error)
        }
    }
}

async fn run_single_summary_job(
    app: &AppHandle,
    job: &LocalQueueJob,
) -> Result<SummaryJobResult, String> {
    db::start_local_queue_pipeline_step("summary", &job.id, "load_material", 10)?;
    let settings = db::get_settings()?;
    let api_key = db::get_llm_api_key()?;
    let transcript_id = job
        .transcript_id
        .as_ref()
        .ok_or_else(|| "Summary job missing transcript_id".to_string())?;
    let template_id = job
        .template_id
        .as_ref()
        .ok_or_else(|| "Summary job missing template_id".to_string())?;
    let transcript = db::get_transcript(transcript_id)?;
    let template = db::get_template(template_id)?;

    db::complete_local_queue_pipeline_step("summary", &job.id, "load_material")?;
    db::start_local_queue_pipeline_step("summary", &job.id, "build_prompt", 25)?;
    db::update_summary_job_progress(&job.id, 35)?;
    emit_current(app, "summary", &job.id);
    db::complete_local_queue_pipeline_step("summary", &job.id, "build_prompt")?;
    db::start_local_queue_pipeline_step("summary", &job.id, "call_llm", 5)?;
    emit_current(app, "summary", &job.id);
    let (content, raw) =
        llm::summarize_transcript_text(&transcript.text, &template.prompt, &settings, &api_key)
            .await?;

    db::update_summary_job_progress(&job.id, 90)?;
    db::complete_local_queue_pipeline_step("summary", &job.id, "call_llm")?;
    db::start_local_queue_pipeline_step("summary", &job.id, "save_result", 10)?;
    emit_current(app, "summary", &job.id);
    let summary = save_summary_record(
        Some(&transcript.id),
        &job.id,
        &template.id,
        Some(template.name),
        content,
        Some(raw.to_string()),
        &settings.llm_provider,
        &settings.llm_model,
    )?;
    Ok(SummaryJobResult {
        job_id: job.id.clone(),
        summary,
    })
}

async fn run_workspace_summary_job(
    app: &AppHandle,
    job: &LocalQueueJob,
) -> Result<SummaryJobResult, String> {
    db::start_local_queue_pipeline_step("summary", &job.id, "load_material", 10)?;
    let settings = db::get_settings()?;
    let api_key = db::get_llm_api_key()?;
    let template_id = job
        .template_id
        .as_ref()
        .ok_or_else(|| "Workspace summary job missing template_id".to_string())?;
    let template = db::get_template(template_id)?;
    let metadata = job.metadata.clone().unwrap_or_else(|| json!({}));
    let transcript_ids = metadata
        .get("transcript_ids")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "Workspace summary job missing transcript_ids".to_string())?
        .iter()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    if transcript_ids.is_empty() {
        return Err("当前工作区没有可总结的转录".to_string());
    }
    let workspace_title = metadata
        .get("workspace_title")
        .and_then(|value| value.as_str())
        .unwrap_or("本地工作区");
    let notes = metadata
        .get("notes")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut transcript_blocks = Vec::new();
    let mut first_transcript: Option<TranscriptRecord> = None;
    for (index, transcript_id) in transcript_ids.iter().enumerate() {
        let transcript = db::get_transcript(transcript_id)?;
        let recording = db::get_recording(&transcript.recording_id)?;
        if first_transcript.is_none() {
            first_transcript = Some(transcript.clone());
        }
        transcript_blocks.push(format!(
            "## 录音：{}\n\n{}",
            recording.title.trim(),
            transcript.text.trim()
        ));
        let progress = 20 + (((index + 1) as f64 / transcript_ids.len() as f64) * 20.0) as i64;
        db::update_summary_job_progress(&job.id, progress)?;
        emit_current(app, "summary", &job.id);
    }

    db::complete_local_queue_pipeline_step("summary", &job.id, "load_material")?;
    db::start_local_queue_pipeline_step("summary", &job.id, "build_prompt", 25)?;
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
    db::update_summary_job_progress(&job.id, 55)?;
    db::complete_local_queue_pipeline_step("summary", &job.id, "build_prompt")?;
    db::start_local_queue_pipeline_step("summary", &job.id, "call_llm", 5)?;
    emit_current(app, "summary", &job.id);
    let (content, raw) =
        llm::summarize_transcript_text(&material, &workspace_template, &settings, &api_key).await?;
    db::update_summary_job_progress(&job.id, 90)?;
    db::complete_local_queue_pipeline_step("summary", &job.id, "call_llm")?;
    db::start_local_queue_pipeline_step("summary", &job.id, "save_result", 10)?;
    emit_current(app, "summary", &job.id);
    let summary = save_summary_record(
        Some(&first_transcript.id),
        &job.id,
        &template.id,
        Some(format!("{} 综合总结", workspace_title.trim())),
        content,
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
    Ok(SummaryJobResult {
        job_id: job.id.clone(),
        summary,
    })
}

async fn run_text_summary_job(
    app: &AppHandle,
    job: &LocalQueueJob,
) -> Result<SummaryJobResult, String> {
    db::start_local_queue_pipeline_step("summary", &job.id, "load_material", 10)?;
    let settings = db::get_settings()?;
    let api_key = db::get_llm_api_key()?;
    let template_id = job
        .template_id
        .as_ref()
        .ok_or_else(|| "Text summary job missing template_id".to_string())?;
    let template = db::get_template(template_id)?;
    let metadata = job.metadata.clone().unwrap_or_else(|| json!({}));
    let documents = metadata
        .get("documents")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "Text summary job missing documents".to_string())?
        .iter()
        .filter_map(|item| {
            let title = item.get("title")?.as_str()?.trim().to_string();
            let content = item.get("content")?.as_str()?.trim().to_string();
            if content.is_empty() {
                return None;
            }
            Some(WorkspaceTextDocument { title, content })
        })
        .collect::<Vec<_>>();
    if documents.is_empty() {
        return Err("当前本地空间没有可总结的文本文件".to_string());
    }

    db::complete_local_queue_pipeline_step("summary", &job.id, "load_material")?;
    db::start_local_queue_pipeline_step("summary", &job.id, "build_prompt", 25)?;
    let title = metadata
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("文本总结");
    let workspace_title = metadata
        .get("workspace_title")
        .and_then(|value| value.as_str())
        .unwrap_or("本地空间");

    let text_blocks = documents
        .iter()
        .enumerate()
        .map(|(index, document)| {
            let progress = 20 + (((index + 1) as f64 / documents.len() as f64) * 20.0) as i64;
            let _ = db::update_summary_job_progress(&job.id, progress);
            emit_current(app, "summary", &job.id);
            let doc_title = document.title.trim();
            format!(
                "## 文本：{}\n\n{}",
                if doc_title.is_empty() {
                    "未命名文本"
                } else {
                    doc_title
                },
                document.content.trim()
            )
        })
        .collect::<Vec<_>>();

    let material = format!(
        "# 本地空间：{}\n\n# 文本材料\n\n{}",
        workspace_title.trim(),
        text_blocks.join("\n\n---\n\n")
    );
    let workspace_template = format!(
        "{}\n\n请将以下同一本地空间下的全部文本文件作为一个整体处理，输出一份综合 Markdown 结果。需要跨文件合并重复信息，按主题/决策/行动项/风险归纳，不要逐文件简单罗列。",
        template.prompt
    );
    db::update_summary_job_progress(&job.id, 55)?;
    db::complete_local_queue_pipeline_step("summary", &job.id, "build_prompt")?;
    db::start_local_queue_pipeline_step("summary", &job.id, "call_llm", 5)?;
    emit_current(app, "summary", &job.id);
    let (content, raw) =
        llm::summarize_transcript_text(&material, &workspace_template, &settings, &api_key).await?;
    db::update_summary_job_progress(&job.id, 90)?;
    db::complete_local_queue_pipeline_step("summary", &job.id, "call_llm")?;
    db::start_local_queue_pipeline_step("summary", &job.id, "save_result", 10)?;
    emit_current(app, "summary", &job.id);
    let summary = save_summary_record(
        None,
        &job.id,
        &template.id,
        Some(title.to_string()),
        content,
        Some(
            json!({
                "strategy": job.summary_scope.as_deref().unwrap_or("workspace_text"),
                "raw": raw
            })
            .to_string(),
        ),
        &settings.llm_provider,
        &settings.llm_model,
    )?;
    Ok(SummaryJobResult {
        job_id: job.id.clone(),
        summary,
    })
}

fn save_summary_record(
    transcript_id: Option<&str>,
    job_id: &str,
    template_id: &str,
    title: Option<String>,
    content: String,
    result_json: Option<String>,
    provider: &str,
    model: &str,
) -> Result<SummaryRecord, String> {
    let summary = db::insert_summary(
        transcript_id,
        job_id,
        template_id,
        title,
        &content,
        result_json,
        provider,
        model,
    )?;
    db::attach_summary_output(job_id, &summary.id)?;
    db::complete_local_queue_pipeline_step("summary", job_id, "save_result")?;
    db::update_summary_job_progress(job_id, 95)?;
    db::start_local_queue_pipeline_step("summary", job_id, "write_files", 20)?;
    super::commands::local::write_summary_files_for_queue(&summary)?;
    db::complete_local_queue_pipeline_step("summary", job_id, "write_files")?;
    Ok(summary)
}

pub async fn enqueue_transcription(
    app: &AppHandle,
    recording_id: String,
    workspace_folder: Option<String>,
    next_summary_template_id: Option<String>,
) -> Result<LocalQueueJob, String> {
    let recording = db::get_recording(&recording_id)?;
    let metadata = json!({
        "recording_title": recording.title,
        "original_audio_path": recording.original_audio_path,
        "next_summary_template_id": next_summary_template_id,
    });
    let job =
        db::enqueue_transcription_job(&recording_id, workspace_folder.as_deref(), None, metadata)?;
    emit_queue_update(app, &job);
    notify_transcription();
    Ok(job)
}

async fn enqueue_followup_summary(
    app: &AppHandle,
    job: &LocalQueueJob,
    transcript: &TranscriptRecord,
) {
    let template_id = job
        .metadata
        .as_ref()
        .and_then(|value| value.get("next_summary_template_id"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let Some(template_id) = template_id else {
        return;
    };
    if let Err(error) = enqueue_summary(
        app,
        transcript.id.clone(),
        template_id,
        job.workspace_folder.clone(),
    )
    .await
    {
        log::error!("Failed to enqueue follow-up summary: {error}");
    }
}

pub async fn enqueue_summary(
    app: &AppHandle,
    transcript_id: String,
    template_id: String,
    workspace_folder: Option<String>,
) -> Result<LocalQueueJob, String> {
    let settings = db::get_settings()?;
    let transcript = db::get_transcript(&transcript_id)?;
    let template = db::get_template(&template_id)?;
    let recording = db::get_recording(&transcript.recording_id)?;
    let metadata = json!({
        "recording_title": recording.title,
        "template_name": template.name,
    });
    let job = db::enqueue_summary_job(
        Some(&transcript_id),
        &template_id,
        &settings.llm_provider,
        &settings.llm_model,
        workspace_folder.as_deref(),
        "single_transcript",
        metadata,
    )?;
    emit_queue_update(app, &job);
    notify_summary();
    Ok(job)
}

pub async fn enqueue_workspace_summary(
    app: &AppHandle,
    transcript_ids: Vec<String>,
    template_id: String,
    workspace_folder: Option<String>,
    workspace_title: String,
    notes: Vec<String>,
) -> Result<LocalQueueJob, String> {
    if transcript_ids.is_empty() {
        return Err("当前工作区没有可总结的转录".to_string());
    }
    let settings = db::get_settings()?;
    let first_transcript = db::get_transcript(&transcript_ids[0])?;
    let template = db::get_template(&template_id)?;
    let metadata = json!({
        "transcript_ids": transcript_ids,
        "workspace_title": workspace_title,
        "template_name": template.name,
        "notes": notes,
    });
    let job = db::enqueue_summary_job(
        Some(&first_transcript.id),
        &template_id,
        &settings.llm_provider,
        &settings.llm_model,
        workspace_folder.as_deref(),
        "workspace",
        metadata,
    )?;
    emit_queue_update(app, &job);
    notify_summary();
    Ok(job)
}

pub async fn enqueue_workspace_text_summary(
    app: &AppHandle,
    template_id: String,
    workspace_folder: Option<String>,
    workspace_title: String,
    documents: Vec<WorkspaceTextDocument>,
    title: String,
    summary_scope: String,
) -> Result<LocalQueueJob, String> {
    let documents = documents
        .into_iter()
        .map(|document| WorkspaceTextDocument {
            title: document.title.trim().to_string(),
            content: document.content.trim().to_string(),
        })
        .filter(|document| !document.content.is_empty())
        .collect::<Vec<_>>();
    if documents.is_empty() {
        return Err("当前本地空间没有可总结的文本文件".to_string());
    }
    let settings = db::get_settings()?;
    let template = db::get_template(&template_id)?;
    let scope = match summary_scope.as_str() {
        "single_text" => "single_text",
        _ => "workspace_text",
    };
    let title = if title.trim().is_empty() {
        if scope == "single_text" {
            format!("总结 - {} - {}", documents[0].title.trim(), template.name)
        } else {
            format!(
                "{} 本地空间总结 - {}",
                workspace_title.trim(),
                template.name
            )
        }
    } else {
        title.trim().to_string()
    };
    let metadata = json!({
        "documents": documents,
        "workspace_title": workspace_title,
        "template_name": template.name,
        "title": title,
    });
    let job = db::enqueue_summary_job(
        None,
        &template_id,
        &settings.llm_provider,
        &settings.llm_model,
        workspace_folder.as_deref(),
        scope,
        metadata,
    )?;
    emit_queue_update(app, &job);
    notify_summary();
    Ok(job)
}

fn should_retry_asr_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("worker exited")
        || lower.contains("broken pipe")
        || lower.contains("failed to write asr worker")
        || lower.contains("failed to read asr worker")
        || lower.contains("invalid asr worker json")
}

fn emit_current(app: &AppHandle, queue_type: &str, job_id: &str) {
    if let Ok(job) = db::get_local_queue_job(queue_type, job_id) {
        emit_queue_update(app, &job);
    }
}

#[allow(dead_code)]
pub fn workspace_summary_result_from_summary(
    job_id: String,
    summary: SummaryRecord,
) -> WorkspaceSummaryResult {
    WorkspaceSummaryResult {
        job_id,
        content: summary.content.clone(),
        summary,
    }
}
