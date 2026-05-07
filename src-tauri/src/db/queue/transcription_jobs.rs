use rusqlite::params;
use serde_json::Value;
use uuid::Uuid;

use crate::types::LocalQueueJob;

use super::super::schema::init_db;
use super::super::{connect, now_iso, DEFAULT_ASR_ENGINE};
use super::local_jobs::get_local_queue_job;
use super::pipeline::{
    complete_pipeline, fail_pipeline, metadata_with_initialized_pipeline, reset_pipeline,
    start_pipeline, transcription_pipeline_progress, update_pipeline_step,
};

pub fn create_transcription_job(
    recording_id: &str,
    model_path: Option<&str>,
) -> Result<String, String> {
    let conn = connect()?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        r#"
        INSERT INTO transcription_jobs
          (id, recording_id, status, engine, model_path, created_at, started_at)
        VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?5)
        "#,
        params![id, recording_id, DEFAULT_ASR_ENGINE, model_path, now_iso()],
    )
    .map_err(|e| format!("Failed to create transcription job: {e}"))?;
    Ok(id)
}

pub fn enqueue_transcription_job(
    recording_id: &str,
    workspace_folder: Option<&str>,
    model_path: Option<&str>,
    metadata: Value,
) -> Result<LocalQueueJob, String> {
    init_db()?;
    let conn = connect()?;
    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    let metadata = metadata_with_initialized_pipeline(metadata, "transcription", None)?;
    conn.execute(
        r#"
        INSERT INTO transcription_jobs
          (id, recording_id, status, engine, model_path, workspace_folder, progress,
           queued_at, created_at, updated_at, retry_count, metadata)
        VALUES (?1, ?2, 'pending', ?3, ?4, ?5, 0, ?6, ?6, ?6, 0, ?7)
        "#,
        params![
            id,
            recording_id,
            DEFAULT_ASR_ENGINE,
            model_path,
            workspace_folder,
            now,
            metadata.to_string()
        ],
    )
    .map_err(|e| format!("Failed to enqueue transcription job: {e}"))?;
    get_local_queue_job("transcription", &id)
}

pub fn mark_transcription_job_running(job_id: &str, progress: i64) -> Result<(), String> {
    let conn = connect()?;
    let now = now_iso();
    conn.execute(
        "UPDATE transcription_jobs SET status = 'running', progress = ?1, started_at = COALESCE(started_at, ?2), updated_at = ?2 WHERE id = ?3",
        params![progress, now, job_id],
    )
    .map_err(|e| format!("Failed to mark transcription running: {e}"))?;
    start_pipeline(&conn, "transcription_jobs", job_id, "transcription", &now)?;
    Ok(())
}

pub fn update_transcription_job_progress(job_id: &str, progress: i64) -> Result<(), String> {
    let conn = connect()?;
    let progress = progress.clamp(0, 100);
    let now = now_iso();
    conn.execute(
        "UPDATE transcription_jobs SET progress = ?1, updated_at = ?2 WHERE id = ?3",
        params![progress, now, job_id],
    )
    .map_err(|e| format!("Failed to update transcription progress: {e}"))?;
    if let Some((step_name, step_progress)) = transcription_pipeline_progress(progress) {
        update_pipeline_step(
            &conn,
            "transcription_jobs",
            job_id,
            "transcription",
            step_name,
            "running",
            step_progress,
            None,
            &now,
            false,
        )?;
    }
    Ok(())
}

pub fn retry_transcription_job(job_id: &str, message: &str) -> Result<(), String> {
    let conn = connect()?;
    let now = now_iso();
    conn.execute(
        "UPDATE transcription_jobs SET retry_count = retry_count + 1, progress = 20, updated_at = ?1, error_code = 'ASR_WORKER_RETRY', error_message = ?2 WHERE id = ?3",
        params![now, message, job_id],
    )
    .map_err(|e| format!("Failed to update transcription retry: {e}"))?;
    reset_pipeline(&conn, "transcription_jobs", job_id, "transcription", &now)?;
    update_pipeline_step(
        &conn,
        "transcription_jobs",
        job_id,
        "transcription",
        "prepare_audio_environment",
        "running",
        60,
        Some(message),
        &now,
        false,
    )?;
    Ok(())
}

pub fn fail_transcription_job(job_id: &str, code: &str, message: &str) -> Result<(), String> {
    let conn = connect()?;
    let now = now_iso();
    conn.execute(
        "UPDATE transcription_jobs SET status = 'failed', progress = 100, finished_at = ?1, updated_at = ?1, error_code = ?2, error_message = ?3 WHERE id = ?4",
        params![now, code, message, job_id],
    )
    .map_err(|e| format!("Failed to mark transcription failed: {e}"))?;
    fail_pipeline(
        &conn,
        "transcription_jobs",
        job_id,
        "transcription",
        message,
        &now,
    )?;
    Ok(())
}

pub fn finish_transcription_job(job_id: &str) -> Result<(), String> {
    let conn = connect()?;
    let now = now_iso();
    conn.execute(
        "UPDATE transcription_jobs SET status = 'succeeded', progress = 100, finished_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![now, job_id],
    )
    .map_err(|e| format!("Failed to mark transcription complete: {e}"))?;
    complete_pipeline(&conn, "transcription_jobs", job_id, "transcription", &now)?;
    Ok(())
}

pub fn attach_transcription_output(job_id: &str, transcript_id: &str) -> Result<(), String> {
    let conn = connect()?;
    conn.execute(
        "UPDATE transcription_jobs SET output_transcript_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![transcript_id, now_iso(), job_id],
    )
    .map_err(|e| format!("Failed to attach transcription output: {e}"))?;
    Ok(())
}
