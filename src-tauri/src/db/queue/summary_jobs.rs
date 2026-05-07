use rusqlite::params;
use serde_json::Value;
use uuid::Uuid;

use crate::types::LocalQueueJob;

use super::super::schema::init_db;
use super::super::{connect, now_iso};
use super::local_jobs::get_local_queue_job;
use super::pipeline::{
    complete_pipeline, fail_pipeline, metadata_with_initialized_pipeline, start_pipeline,
    summary_pipeline_progress, update_pipeline_step,
};

pub fn create_summary_job(
    transcript_id: &str,
    template_id: &str,
    provider: &str,
    model: &str,
) -> Result<String, String> {
    let conn = connect()?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        r#"
        INSERT INTO summary_jobs
          (id, transcript_id, template_id, status, provider, model, created_at, started_at)
        VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6, ?6)
        "#,
        params![id, transcript_id, template_id, provider, model, now_iso()],
    )
    .map_err(|e| format!("Failed to create summary job: {e}"))?;
    Ok(id)
}

pub fn enqueue_summary_job(
    transcript_id: Option<&str>,
    template_id: &str,
    provider: &str,
    model: &str,
    workspace_folder: Option<&str>,
    summary_scope: &str,
    metadata: Value,
) -> Result<LocalQueueJob, String> {
    init_db()?;
    let conn = connect()?;
    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    let metadata = metadata_with_initialized_pipeline(metadata, "summary", None)?;
    conn.execute(
        r#"
        INSERT INTO summary_jobs
          (id, transcript_id, template_id, status, provider, model, workspace_folder,
           progress, queued_at, created_at, updated_at, retry_count, summary_scope, metadata)
        VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, 0, ?7, ?7, ?7, 0, ?8, ?9)
        "#,
        params![
            id,
            transcript_id,
            template_id,
            provider,
            model,
            workspace_folder,
            now,
            summary_scope,
            metadata.to_string()
        ],
    )
    .map_err(|e| format!("Failed to enqueue summary job: {e}"))?;
    get_local_queue_job("summary", &id)
}

pub fn mark_summary_job_running(job_id: &str, progress: i64) -> Result<(), String> {
    let conn = connect()?;
    let now = now_iso();
    conn.execute(
        "UPDATE summary_jobs SET status = 'running', progress = ?1, started_at = COALESCE(started_at, ?2), updated_at = ?2 WHERE id = ?3",
        params![progress, now, job_id],
    )
    .map_err(|e| format!("Failed to mark summary running: {e}"))?;
    start_pipeline(&conn, "summary_jobs", job_id, "summary", &now)?;
    Ok(())
}

pub fn update_summary_job_progress(job_id: &str, progress: i64) -> Result<(), String> {
    let conn = connect()?;
    let progress = progress.clamp(0, 100);
    let now = now_iso();
    conn.execute(
        "UPDATE summary_jobs SET progress = ?1, updated_at = ?2 WHERE id = ?3",
        params![progress, now, job_id],
    )
    .map_err(|e| format!("Failed to update summary progress: {e}"))?;
    if let Some((step_name, step_progress)) = summary_pipeline_progress(progress) {
        update_pipeline_step(
            &conn,
            "summary_jobs",
            job_id,
            "summary",
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

pub fn fail_summary_job(job_id: &str, code: &str, message: &str) -> Result<(), String> {
    let conn = connect()?;
    let now = now_iso();
    conn.execute(
        "UPDATE summary_jobs SET status = 'failed', progress = 100, finished_at = ?1, updated_at = ?1, error_code = ?2, error_message = ?3 WHERE id = ?4",
        params![now, code, message, job_id],
    )
    .map_err(|e| format!("Failed to mark summary failed: {e}"))?;
    fail_pipeline(&conn, "summary_jobs", job_id, "summary", message, &now)?;
    Ok(())
}

pub fn finish_summary_job(job_id: &str) -> Result<(), String> {
    let conn = connect()?;
    let now = now_iso();
    conn.execute(
        "UPDATE summary_jobs SET status = 'succeeded', progress = 100, finished_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![now, job_id],
    )
    .map_err(|e| format!("Failed to mark summary complete: {e}"))?;
    complete_pipeline(&conn, "summary_jobs", job_id, "summary", &now)?;
    Ok(())
}

pub fn attach_summary_output(job_id: &str, summary_id: &str) -> Result<(), String> {
    let conn = connect()?;
    conn.execute(
        "UPDATE summary_jobs SET output_summary_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![summary_id, now_iso(), job_id],
    )
    .map_err(|e| format!("Failed to attach summary output: {e}"))?;
    Ok(())
}
