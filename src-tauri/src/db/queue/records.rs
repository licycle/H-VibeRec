use rusqlite::params;
use uuid::Uuid;

use crate::types::{SummaryRecord, TranscriptRecord};

use super::super::{connect, now_iso};

pub fn insert_transcript(
    recording_id: &str,
    job_id: &str,
    text: &str,
    result_json: &str,
    language: Option<String>,
    confidence: Option<f64>,
    duration_ms: Option<i64>,
    rtf: Option<f64>,
) -> Result<TranscriptRecord, String> {
    let conn = connect()?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        r#"
        INSERT INTO transcripts
          (id, recording_id, job_id, text, result_json, language, confidence, duration_ms, rtf, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
        params![
            id,
            recording_id,
            job_id,
            text,
            result_json,
            language,
            confidence,
            duration_ms,
            rtf,
            now_iso()
        ],
    )
    .map_err(|e| format!("Failed to save transcript: {e}"))?;
    get_transcript(&id)
}

pub fn get_transcript(id: &str) -> Result<TranscriptRecord, String> {
    let conn = connect()?;
    conn.query_row(
        "SELECT id, recording_id, job_id, text, result_json, language, confidence, duration_ms, rtf, created_at FROM transcripts WHERE id = ?1",
        params![id],
        |row| {
            Ok(TranscriptRecord {
                id: row.get(0)?,
                recording_id: row.get(1)?,
                job_id: row.get(2)?,
                text: row.get(3)?,
                result_json: row.get(4)?,
                language: row.get(5)?,
                confidence: row.get(6)?,
                duration_ms: row.get(7)?,
                rtf: row.get(8)?,
                created_at: row.get(9)?,
            })
        },
    )
    .map_err(|e| format!("Transcript not found {id}: {e}"))
}

pub fn latest_transcript_for_recording(recording_id: &str) -> Result<TranscriptRecord, String> {
    let conn = connect()?;
    let id = conn
        .query_row(
            "SELECT id FROM transcripts WHERE recording_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![recording_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|e| format!("No transcript for recording {recording_id}: {e}"))?;
    get_transcript(&id)
}

pub fn insert_summary(
    transcript_id: Option<&str>,
    job_id: &str,
    template_id: &str,
    title: Option<String>,
    content: &str,
    result_json: Option<String>,
    provider: &str,
    model: &str,
) -> Result<SummaryRecord, String> {
    let conn = connect()?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        r#"
        INSERT INTO summaries
          (id, transcript_id, job_id, template_id, title, content, result_json, provider, model, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
        "#,
        params![
            id,
            transcript_id,
            job_id,
            template_id,
            title,
            content,
            result_json,
            provider,
            model,
            now_iso()
        ],
    )
    .map_err(|e| format!("Failed to save summary: {e}"))?;
    get_summary(&id)
}

pub fn get_summary(id: &str) -> Result<SummaryRecord, String> {
    let conn = connect()?;
    conn.query_row(
        "SELECT id, transcript_id, job_id, template_id, title, content, result_json, provider, model, created_at, updated_at FROM summaries WHERE id = ?1",
        params![id],
        |row| {
            Ok(SummaryRecord {
                id: row.get(0)?,
                transcript_id: row.get(1)?,
                job_id: row.get(2)?,
                template_id: row.get(3)?,
                title: row.get(4)?,
                content: row.get(5)?,
                result_json: row.get(6)?,
                provider: row.get(7)?,
                model: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        },
    )
    .map_err(|e| format!("Summary not found {id}: {e}"))
}
