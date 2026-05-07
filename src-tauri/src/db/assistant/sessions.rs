use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::types::AssistantSession;

use super::super::schema::init_db;
use super::super::{connect, now_iso};

#[cfg_attr(not(test), allow(dead_code))]
pub fn create_assistant_session(title: Option<String>) -> Result<AssistantSession, String> {
    init_db()?;
    let conn = connect()?;
    create_assistant_session_with_conn(&conn, title)
}

fn create_assistant_session_with_conn(
    conn: &Connection,
    title: Option<String>,
) -> Result<AssistantSession, String> {
    let id = Uuid::new_v4().to_string();
    let title = title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "AI问答会话".to_string());
    let now = now_iso();
    conn.execute(
        "INSERT INTO assistant_sessions (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![id, title, now],
    )
    .map_err(|e| format!("Failed to create assistant session: {e}"))?;
    assistant_session_with_conn(conn, &id)
}

pub fn get_or_create_assistant_session(
    session_id: Option<&str>,
    title: Option<String>,
) -> Result<AssistantSession, String> {
    init_db()?;
    let conn = connect()?;
    match session_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(id) => assistant_session_with_conn(&conn, id),
        None => create_assistant_session_with_conn(&conn, title),
    }
}

pub fn list_assistant_sessions() -> Result<Vec<AssistantSession>, String> {
    init_db()?;
    let conn = connect()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, created_at, updated_at FROM assistant_sessions ORDER BY updated_at DESC",
        )
        .map_err(|e| format!("Failed to query assistant sessions: {e}"))?;
    let rows = stmt
        .query_map([], row_to_assistant_session)
        .map_err(|e| format!("Failed to read assistant sessions: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode assistant sessions: {e}"))
}

pub fn get_assistant_session(session_id: &str) -> Result<AssistantSession, String> {
    init_db()?;
    let conn = connect()?;
    assistant_session_with_conn(&conn, session_id)
}

fn assistant_session_with_conn(
    conn: &Connection,
    session_id: &str,
) -> Result<AssistantSession, String> {
    if session_id.trim().is_empty() {
        return Err("sessionId is required".to_string());
    }
    conn.query_row(
        "SELECT id, title, created_at, updated_at FROM assistant_sessions WHERE id = ?1",
        params![session_id.trim()],
        row_to_assistant_session,
    )
    .map_err(|e| format!("Assistant session not found {session_id}: {e}"))
}

fn row_to_assistant_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssistantSession> {
    Ok(AssistantSession {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

pub fn delete_assistant_session(session_id: &str) -> Result<(), String> {
    init_db()?;
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err("sessionId is required".to_string());
    }
    let conn = connect()?;
    clear_assistant_agent_session_with_conn(&conn, session_id)?;
    let changed = conn
        .execute(
            "DELETE FROM assistant_sessions WHERE id = ?1",
            params![session_id],
        )
        .map_err(|e| format!("Failed to delete assistant session: {e}"))?;
    if changed == 0 {
        return Err(format!("Assistant session not found: {session_id}"));
    }
    Ok(())
}

pub(super) fn clear_assistant_agent_session_with_conn(
    conn: &Connection,
    session_id: &str,
) -> Result<(), String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Ok(());
    }
    conn.execute(
        "DELETE FROM assistant_agent_items WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(|e| format!("Failed to clear assistant agent items: {e}"))?;
    conn.execute(
        "DELETE FROM assistant_agent_sessions WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(|e| format!("Failed to clear assistant agent session: {e}"))?;
    Ok(())
}

pub(super) fn assistant_session_exists(
    conn: &Connection,
    session_id: &str,
) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM assistant_sessions WHERE id = ?1",
        params![session_id.trim()],
        |_| Ok(()),
    )
    .optional()
    .map(|value| value.is_some())
    .map_err(|e| format!("Failed to check assistant session: {e}"))
}
