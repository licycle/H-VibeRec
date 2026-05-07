use std::path::Path;

use chrono::Local;
use rusqlite::params;
use uuid::Uuid;

use crate::types::LocalRecording;

use super::schema::init_db;
use super::{connect, now_iso};

pub fn recording_id_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    if let Some(workspace_folder) = workspace_folder_from_recording_path(path) {
        format!("{workspace_folder}__{stem}")
    } else {
        stem
    }
}

fn workspace_folder_from_recording_path(path: &Path) -> Option<String> {
    let recordings_dir = path.parent()?;
    if recordings_dir.file_name()?.to_str()? != "recordings" {
        return None;
    }
    let workspace_dir = recordings_dir.parent()?;
    let workspaces_dir = workspace_dir.parent()?;
    if workspaces_dir.file_name()?.to_str()? != "workspaces" {
        return None;
    }
    workspace_dir
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_string())
}

pub fn register_recording(path: &Path, title: Option<String>) -> Result<LocalRecording, String> {
    init_db()?;
    let conn = connect()?;
    let id = recording_id_from_path(path);
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("Failed to read recording metadata {}: {e}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Recording");
    let created_at = metadata
        .created()
        .or_else(|_| metadata.modified())
        .map(|time| {
            let dt: chrono::DateTime<Local> = time.into();
            dt.to_rfc3339()
        })
        .unwrap_or_else(|_| now_iso());
    let title = title.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(file_name)
            .to_string()
    });

    conn.execute(
        r#"
        INSERT INTO recordings
          (id, title, original_audio_path, file_size_bytes, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(id) DO UPDATE SET
          original_audio_path = excluded.original_audio_path,
          file_size_bytes = excluded.file_size_bytes,
          updated_at = excluded.updated_at
        "#,
        params![
            id,
            title,
            path.to_string_lossy().to_string(),
            metadata.len() as i64,
            created_at,
            now_iso()
        ],
    )
    .map_err(|e| format!("Failed to register recording: {e}"))?;
    get_recording(&id)
}

pub fn list_recordings() -> Result<Vec<LocalRecording>, String> {
    init_db()?;
    let conn = connect()?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT
              r.id,
              r.title,
              r.original_audio_path,
              r.normalized_audio_path,
              r.duration_ms,
              r.file_size_bytes,
              r.created_at,
              r.updated_at,
              COALESCE((
                SELECT tj.status FROM transcription_jobs tj
                WHERE tj.recording_id = r.id
                ORDER BY COALESCE(tj.finished_at, tj.started_at, tj.created_at) DESC
                LIMIT 1
              ), 'not_started') AS transcription_status,
              COALESCE((
                SELECT sj.status FROM summary_jobs sj
                JOIN transcripts t ON t.id = sj.transcript_id
                WHERE t.recording_id = r.id
                ORDER BY COALESCE(sj.finished_at, sj.started_at, sj.created_at) DESC
                LIMIT 1
              ), 'not_started') AS summary_status,
              (
                SELECT t.id FROM transcripts t
                WHERE t.recording_id = r.id
                ORDER BY t.created_at DESC
                LIMIT 1
              ) AS latest_transcript_id,
              (
                SELECT s.id FROM summaries s
                JOIN transcripts t ON t.id = s.transcript_id
                WHERE t.recording_id = r.id
                ORDER BY s.created_at DESC
                LIMIT 1
              ) AS latest_summary_id
            FROM recordings r
            ORDER BY r.created_at DESC
            "#,
        )
        .map_err(|e| format!("Failed to query recordings: {e}"))?;

    let rows = stmt
        .query_map([], row_to_local_recording)
        .map_err(|e| format!("Failed to read recordings: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode recordings: {e}"))
}

pub fn get_recording(id: &str) -> Result<LocalRecording, String> {
    let conn = connect()?;
    conn.query_row(
        r#"
        SELECT
          r.id,
          r.title,
          r.original_audio_path,
          r.normalized_audio_path,
          r.duration_ms,
          r.file_size_bytes,
          r.created_at,
          r.updated_at,
          COALESCE((SELECT status FROM transcription_jobs WHERE recording_id = r.id ORDER BY created_at DESC LIMIT 1), 'not_started'),
          COALESCE((
            SELECT sj.status FROM summary_jobs sj
            JOIN transcripts t ON t.id = sj.transcript_id
            WHERE t.recording_id = r.id
            ORDER BY sj.created_at DESC LIMIT 1
          ), 'not_started'),
          (SELECT id FROM transcripts WHERE recording_id = r.id ORDER BY created_at DESC LIMIT 1),
          (
            SELECT s.id FROM summaries s
            JOIN transcripts t ON t.id = s.transcript_id
            WHERE t.recording_id = r.id
            ORDER BY s.created_at DESC LIMIT 1
          )
        FROM recordings r
        WHERE r.id = ?1
        "#,
        params![id],
        row_to_local_recording,
    )
    .map_err(|e| format!("Recording not found {id}: {e}"))
}

fn row_to_local_recording(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalRecording> {
    Ok(LocalRecording {
        id: row.get(0)?,
        title: row.get(1)?,
        original_audio_path: row.get(2)?,
        normalized_audio_path: row.get(3)?,
        duration_ms: row.get(4)?,
        file_size_bytes: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        transcription_status: row.get(8)?,
        summary_status: row.get(9)?,
        latest_transcript_id: row.get(10)?,
        latest_summary_id: row.get(11)?,
    })
}

pub fn delete_recording(id: &str) -> Result<(), String> {
    init_db()?;
    let recording = get_recording(id)?;
    if Path::new(&recording.original_audio_path).exists() {
        std::fs::remove_file(&recording.original_audio_path)
            .map_err(|e| format!("Failed to delete audio file: {e}"))?;
    }
    if let Some(path) = recording.normalized_audio_path {
        if Path::new(&path).exists() {
            let _ = std::fs::remove_file(path);
        }
    }
    let conn = connect()?;
    conn.execute("DELETE FROM recordings WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete recording: {e}"))?;
    Ok(())
}

pub fn update_recording_normalized_path(
    id: &str,
    normalized_path: &Path,
    duration_ms: Option<i64>,
) -> Result<(), String> {
    let conn = connect()?;
    conn.execute(
        "UPDATE recordings SET normalized_audio_path = ?1, duration_ms = COALESCE(?2, duration_ms), updated_at = ?3 WHERE id = ?4",
        params![normalized_path.to_string_lossy().to_string(), duration_ms, now_iso(), id],
    )
    .map_err(|e| format!("Failed to update recording normalized path: {e}"))?;
    Ok(())
}
