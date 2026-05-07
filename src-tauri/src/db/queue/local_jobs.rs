use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::types::LocalQueueJob;

use super::super::schema::init_db;
use super::super::{connect, now_iso};
use super::pipeline::{complete_pipeline, reset_pipeline};

pub fn pending_transcription_jobs() -> Result<Vec<LocalQueueJob>, String> {
    init_db()?;
    list_local_queue_jobs(None, Some("transcription"), Some("pending"))
}

pub fn pending_summary_jobs() -> Result<Vec<LocalQueueJob>, String> {
    init_db()?;
    list_local_queue_jobs(None, Some("summary"), Some("pending"))
}

pub fn recover_interrupted_queue_jobs() -> Result<(), String> {
    init_db()?;
    let conn = connect()?;
    let now = now_iso();
    let completed_transcription_ids = queue_job_ids_where(
        &conn,
        "transcription_jobs",
        "status = 'running' AND output_transcript_id IS NOT NULL",
    )?;
    let requeued_transcription_ids = queue_job_ids_where(
        &conn,
        "transcription_jobs",
        "status = 'running' AND output_transcript_id IS NULL",
    )?;
    let completed_summary_ids = queue_job_ids_where(
        &conn,
        "summary_jobs",
        "status = 'running' AND output_summary_id IS NOT NULL",
    )?;
    let requeued_summary_ids = queue_job_ids_where(
        &conn,
        "summary_jobs",
        "status = 'running' AND output_summary_id IS NULL",
    )?;
    conn.execute(
        r#"
        UPDATE transcription_jobs
        SET status = 'succeeded',
            progress = 100,
            finished_at = COALESCE(finished_at, ?1),
            updated_at = ?1
        WHERE status = 'running'
          AND output_transcript_id IS NOT NULL
        "#,
        params![now],
    )
    .map_err(|e| format!("Failed to complete recovered transcription queue jobs: {e}"))?;
    conn.execute(
        r#"
        UPDATE transcription_jobs
        SET status = 'pending',
            progress = 0,
            started_at = NULL,
            updated_at = ?1,
            error_code = 'QUEUE_RECOVERED',
            error_message = '应用重启后自动恢复未完成的转录任务'
        WHERE status = 'running'
          AND output_transcript_id IS NULL
        "#,
        params![now],
    )
    .map_err(|e| format!("Failed to recover transcription queue jobs: {e}"))?;
    conn.execute(
        r#"
        UPDATE summary_jobs
        SET status = 'succeeded',
            progress = 100,
            finished_at = COALESCE(finished_at, ?1),
            updated_at = ?1
        WHERE status = 'running'
          AND output_summary_id IS NOT NULL
        "#,
        params![now],
    )
    .map_err(|e| format!("Failed to complete recovered summary queue jobs: {e}"))?;
    conn.execute(
        r#"
        UPDATE summary_jobs
        SET status = 'pending',
            progress = 0,
            started_at = NULL,
            updated_at = ?1,
            error_code = 'QUEUE_RECOVERED',
            error_message = '应用重启后自动恢复未完成的总结任务'
        WHERE status = 'running'
          AND output_summary_id IS NULL
        "#,
        params![now],
    )
    .map_err(|e| format!("Failed to recover summary queue jobs: {e}"))?;
    complete_pipeline_jobs(
        &conn,
        "transcription_jobs",
        "transcription",
        &completed_transcription_ids,
        &now,
    )?;
    reset_pipeline_jobs(
        &conn,
        "transcription_jobs",
        "transcription",
        &requeued_transcription_ids,
        &now,
    )?;
    complete_pipeline_jobs(
        &conn,
        "summary_jobs",
        "summary",
        &completed_summary_ids,
        &now,
    )?;
    reset_pipeline_jobs(
        &conn,
        "summary_jobs",
        "summary",
        &requeued_summary_ids,
        &now,
    )?;
    Ok(())
}

pub fn list_local_queue_jobs(
    workspace_folder: Option<&str>,
    queue_type: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<LocalQueueJob>, String> {
    init_db()?;
    let mut jobs = Vec::new();
    if queue_type.is_none() || queue_type == Some("transcription") {
        jobs.extend(list_transcription_queue_jobs(workspace_folder, status)?);
    }
    if queue_type.is_none() || queue_type == Some("summary") {
        jobs.extend(list_summary_queue_jobs(workspace_folder, status)?);
    }
    jobs.sort_by(|a, b| {
        let left = a.updated_at.as_ref().unwrap_or(&a.created_at);
        let right = b.updated_at.as_ref().unwrap_or(&b.created_at);
        right.cmp(left)
    });
    Ok(jobs)
}

pub fn get_local_queue_job(queue_type: &str, job_id: &str) -> Result<LocalQueueJob, String> {
    match queue_type {
        "transcription" => get_transcription_queue_job(job_id),
        "summary" => get_summary_queue_job(job_id),
        _ => Err(format!("Unknown queue type: {queue_type}")),
    }
}

pub fn cancel_local_queue_job(queue_type: &str, job_id: &str) -> Result<LocalQueueJob, String> {
    let conn = connect()?;
    let table = match queue_type {
        "transcription" => "transcription_jobs",
        "summary" => "summary_jobs",
        _ => return Err(format!("Unknown queue type: {queue_type}")),
    };
    let now = now_iso();
    let affected = conn
        .execute(
            &format!(
                "UPDATE {table} SET status = 'cancelled', progress = 100, cancelled_at = ?1, finished_at = ?1, updated_at = ?1 WHERE id = ?2 AND status = 'pending'"
            ),
            params![now, job_id],
        )
        .map_err(|e| format!("Failed to cancel local queue job: {e}"))?;
    if affected == 0 {
        let job = get_local_queue_job(queue_type, job_id)?;
        if job.status == "pending" {
            return Err("任务取消失败".to_string());
        }
        return Err("只能取消排队中的任务".to_string());
    }
    get_local_queue_job(queue_type, job_id)
}

pub fn mark_local_queue_job_synced(
    queue_type: &str,
    job_id: &str,
) -> Result<LocalQueueJob, String> {
    let conn = connect()?;
    let table = match queue_type {
        "transcription" => "transcription_jobs",
        "summary" => "summary_jobs",
        _ => return Err(format!("Unknown queue type: {queue_type}")),
    };
    let metadata_text: Option<String> = conn
        .query_row(
            &format!("SELECT metadata FROM {table} WHERE id = ?1"),
            params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed to read local queue job metadata: {e}"))?
        .ok_or_else(|| format!("Local queue job not found: {job_id}"))?;
    let mut metadata = parse_json_option(metadata_text).unwrap_or_else(|| serde_json::json!({}));
    if !metadata.is_object() {
        metadata = serde_json::json!({});
    }
    if let Some(object) = metadata.as_object_mut() {
        object.insert("frontend_synced_at".to_string(), Value::String(now_iso()));
    }
    conn.execute(
        &format!("UPDATE {table} SET metadata = ?1, updated_at = ?2 WHERE id = ?3"),
        params![metadata.to_string(), now_iso(), job_id],
    )
    .map_err(|e| format!("Failed to mark local queue job synced: {e}"))?;
    get_local_queue_job(queue_type, job_id)
}

fn queue_job_ids_where(
    conn: &Connection,
    table: &str,
    predicate: &str,
) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(&format!("SELECT id FROM {table} WHERE {predicate}"))
        .map_err(|e| format!("Failed to query local queue jobs: {e}"))?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to read local queue jobs: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode local queue jobs: {e}"))?;
    Ok(ids)
}

fn complete_pipeline_jobs(
    conn: &Connection,
    table: &str,
    queue_type: &str,
    job_ids: &[String],
    now: &str,
) -> Result<(), String> {
    for id in job_ids {
        complete_pipeline(conn, table, id, queue_type, now)?;
    }
    Ok(())
}

fn reset_pipeline_jobs(
    conn: &Connection,
    table: &str,
    queue_type: &str,
    job_ids: &[String],
    now: &str,
) -> Result<(), String> {
    for id in job_ids {
        reset_pipeline(conn, table, id, queue_type, now)?;
    }
    Ok(())
}

fn list_transcription_queue_jobs(
    workspace_folder: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<LocalQueueJob>, String> {
    let conn = connect()?;
    let mut sql = transcription_queue_select_sql().to_string();
    sql.push_str(" WHERE 1=1");
    if workspace_folder.is_some() {
        sql.push_str(" AND tj.workspace_folder = ?1");
    }
    if status.is_some() {
        sql.push_str(if workspace_folder.is_some() {
            " AND tj.status = ?2"
        } else {
            " AND tj.status = ?1"
        });
    }
    sql.push_str(" ORDER BY COALESCE(tj.updated_at, tj.created_at) DESC");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed to query transcription queue jobs: {e}"))?;
    let rows = match (workspace_folder, status) {
        (Some(workspace), Some(status)) => {
            stmt.query_map(params![workspace, status], row_to_transcription_queue_job)
        }
        (Some(workspace), None) => {
            stmt.query_map(params![workspace], row_to_transcription_queue_job)
        }
        (None, Some(status)) => stmt.query_map(params![status], row_to_transcription_queue_job),
        (None, None) => stmt.query_map([], row_to_transcription_queue_job),
    }
    .map_err(|e| format!("Failed to read transcription queue jobs: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode transcription queue jobs: {e}"))
}

fn list_summary_queue_jobs(
    workspace_folder: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<LocalQueueJob>, String> {
    let conn = connect()?;
    let mut sql = summary_queue_select_sql().to_string();
    sql.push_str(" WHERE 1=1");
    if workspace_folder.is_some() {
        sql.push_str(" AND sj.workspace_folder = ?1");
    }
    if status.is_some() {
        sql.push_str(if workspace_folder.is_some() {
            " AND sj.status = ?2"
        } else {
            " AND sj.status = ?1"
        });
    }
    sql.push_str(" ORDER BY COALESCE(sj.updated_at, sj.created_at) DESC");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed to query summary queue jobs: {e}"))?;
    let rows = match (workspace_folder, status) {
        (Some(workspace), Some(status)) => {
            stmt.query_map(params![workspace, status], row_to_summary_queue_job)
        }
        (Some(workspace), None) => stmt.query_map(params![workspace], row_to_summary_queue_job),
        (None, Some(status)) => stmt.query_map(params![status], row_to_summary_queue_job),
        (None, None) => stmt.query_map([], row_to_summary_queue_job),
    }
    .map_err(|e| format!("Failed to read summary queue jobs: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode summary queue jobs: {e}"))
}

pub(super) fn get_transcription_queue_job(job_id: &str) -> Result<LocalQueueJob, String> {
    let conn = connect()?;
    conn.query_row(
        &format!("{} WHERE tj.id = ?1", transcription_queue_select_sql()),
        params![job_id],
        row_to_transcription_queue_job,
    )
    .map_err(|e| format!("Transcription queue job not found {job_id}: {e}"))
}

pub(super) fn get_summary_queue_job(job_id: &str) -> Result<LocalQueueJob, String> {
    let conn = connect()?;
    conn.query_row(
        &format!("{} WHERE sj.id = ?1", summary_queue_select_sql()),
        params![job_id],
        row_to_summary_queue_job,
    )
    .map_err(|e| format!("Summary queue job not found {job_id}: {e}"))
}

fn transcription_queue_select_sql() -> &'static str {
    r#"
    SELECT
      tj.id, tj.status, tj.progress, tj.workspace_folder, tj.created_at, tj.queued_at,
      tj.started_at, tj.finished_at, tj.updated_at, tj.error_message, tj.retry_count,
      tj.recording_id, tj.model_path, tj.output_transcript_id, tj.metadata, r.title
    FROM transcription_jobs tj
    JOIN recordings r ON r.id = tj.recording_id
    "#
}

fn summary_queue_select_sql() -> &'static str {
    r#"
    SELECT
      sj.id, sj.status, sj.progress, sj.workspace_folder, sj.created_at, sj.queued_at,
      sj.started_at, sj.finished_at, sj.updated_at, sj.error_message, sj.retry_count,
      sj.transcript_id, sj.template_id, sj.summary_scope, sj.output_summary_id, sj.metadata,
      st.name, COALESCE(r.title, '')
    FROM summary_jobs sj
    JOIN summary_templates st ON st.id = sj.template_id
    LEFT JOIN transcripts t ON t.id = sj.transcript_id
    LEFT JOIN recordings r ON r.id = t.recording_id
    "#
}

fn row_to_transcription_queue_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalQueueJob> {
    let metadata_text: Option<String> = row.get(14)?;
    let recording_title: String = row.get(15)?;
    Ok(LocalQueueJob {
        id: row.get(0)?,
        queue_type: "transcription".to_string(),
        status: row.get(1)?,
        progress: row.get(2)?,
        title: format!("转录 - {recording_title}"),
        subtitle: Some(recording_title),
        workspace_folder: row.get(3)?,
        created_at: row.get(4)?,
        queued_at: row.get(5)?,
        started_at: row.get(6)?,
        finished_at: row.get(7)?,
        updated_at: row.get(8)?,
        error_message: row.get(9)?,
        retry_count: row.get(10)?,
        recording_id: row.get(11)?,
        transcript_id: None,
        template_id: None,
        summary_scope: None,
        output_transcript_id: row.get(13)?,
        output_summary_id: None,
        metadata: parse_json_option(metadata_text),
    })
}

fn row_to_summary_queue_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalQueueJob> {
    let metadata_text: Option<String> = row.get(15)?;
    let template_name: String = row.get(16)?;
    let recording_title: String = row.get(17)?;
    let scope: String = row.get(13)?;
    let metadata_value = parse_json_option(metadata_text.clone());
    let title = if scope == "workspace" {
        metadata_text
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| {
                value
                    .get("workspace_title")
                    .and_then(|item| item.as_str())
                    .map(|value| format!("{value} 综合总结"))
            })
            .unwrap_or_else(|| "综合总结".to_string())
    } else if scope == "workspace_text" || scope == "single_text" {
        metadata_value
            .as_ref()
            .and_then(|value| value.get("title"))
            .and_then(|item| item.as_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "文本总结".to_string())
    } else {
        format!("总结 - {recording_title}")
    };
    Ok(LocalQueueJob {
        id: row.get(0)?,
        queue_type: "summary".to_string(),
        status: row.get(1)?,
        progress: row.get(2)?,
        title,
        subtitle: Some(template_name),
        workspace_folder: row.get(3)?,
        created_at: row.get(4)?,
        queued_at: row.get(5)?,
        started_at: row.get(6)?,
        finished_at: row.get(7)?,
        updated_at: row.get(8)?,
        error_message: row.get(9)?,
        retry_count: row.get(10)?,
        recording_id: None,
        transcript_id: row.get(11)?,
        template_id: row.get(12)?,
        summary_scope: Some(scope),
        output_transcript_id: None,
        output_summary_id: row.get(14)?,
        metadata: metadata_value,
    })
}

pub(super) fn parse_json_option(text: Option<String>) -> Option<Value> {
    text.and_then(|value| serde_json::from_str::<Value>(&value).ok())
}
