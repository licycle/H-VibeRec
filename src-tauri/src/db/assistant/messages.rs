use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::types::{AssistantMessage, AssistantSource};

use super::super::schema::init_db;
use super::super::{connect, now_iso};
use super::scope::validate_assistant_scope;
use super::sessions::{assistant_session_exists, clear_assistant_agent_session_with_conn};

#[allow(clippy::too_many_arguments)]
pub fn insert_assistant_message(
    session_id: &str,
    role: &str,
    content: &str,
    scope: &str,
    workspace_folder: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
    sources: &[AssistantSource],
) -> Result<AssistantMessage, String> {
    init_db()?;
    validate_assistant_message(role, content, scope, workspace_folder, sources)?;
    let conn = connect()?;
    if !assistant_session_exists(&conn, session_id)? {
        return Err(format!("Assistant session not found: {session_id}"));
    }

    let id = Uuid::new_v4().to_string();
    let now = now_iso();
    let sources_json = serde_json::to_string(sources)
        .map_err(|e| format!("Failed to serialize assistant sources: {e}"))?;
    conn.execute(
        r#"
        INSERT INTO assistant_messages
          (id, session_id, role, content, scope, workspace_folder, provider, model, sources_json, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
        params![
            id,
            session_id.trim(),
            role.trim(),
            content.trim(),
            scope.trim(),
            workspace_folder.map(str::trim).filter(|value| !value.is_empty()),
            provider.map(str::trim).filter(|value| !value.is_empty()),
            model.map(str::trim).filter(|value| !value.is_empty()),
            sources_json,
            now
        ],
    )
    .map_err(|e| format!("Failed to save assistant message: {e}"))?;
    conn.execute(
        "UPDATE assistant_sessions SET updated_at = ?1 WHERE id = ?2",
        params![now, session_id.trim()],
    )
    .map_err(|e| format!("Failed to touch assistant session: {e}"))?;
    get_assistant_message(&id)
}

fn validate_assistant_message(
    role: &str,
    content: &str,
    scope: &str,
    workspace_folder: Option<&str>,
    sources: &[AssistantSource],
) -> Result<(), String> {
    match role.trim() {
        "user" | "assistant" => {}
        _ => return Err("assistant message role must be user or assistant".to_string()),
    }
    if content.trim().is_empty() {
        return Err("assistant message content is required".to_string());
    }
    validate_assistant_scope(scope, workspace_folder)?;
    for source in sources {
        let source_type = source.source_type.as_deref().unwrap_or("note").trim();
        match source_type {
            "web" => {
                if source.id.trim().is_empty() || source.title.trim().is_empty() {
                    return Err("assistant web source id and title are required".to_string());
                }
                let url = source
                    .url
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(source.id.trim());
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    return Err("assistant web source url must be http or https".to_string());
                }
            }
            "note" | "" => {
                if source.id.trim().is_empty()
                    || source.note_id.trim().is_empty()
                    || source.title.trim().is_empty()
                {
                    return Err("assistant source id, note_id and title are required".to_string());
                }
                if let (Some(start), Some(end)) = (source.start_line, source.end_line) {
                    if start == 0 || end < start {
                        return Err("assistant source line range is invalid".to_string());
                    }
                }
            }
            _ => return Err("assistant source type must be note or web".to_string()),
        }
    }
    Ok(())
}

pub fn list_assistant_messages(session_id: &str) -> Result<Vec<AssistantMessage>, String> {
    init_db()?;
    if session_id.trim().is_empty() {
        return Err("sessionId is required".to_string());
    }
    let conn = connect()?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, session_id, role, content, scope, workspace_folder, provider, model, sources_json, created_at
            FROM assistant_messages
            WHERE session_id = ?1
            ORDER BY created_at ASC
            "#,
        )
        .map_err(|e| format!("Failed to query assistant messages: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.trim()], row_to_assistant_message)
        .map_err(|e| format!("Failed to read assistant messages: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode assistant messages: {e}"))
}

fn assistant_message_position(
    conn: &Connection,
    message_id: &str,
) -> Result<(i64, String, String), String> {
    let message_id = message_id.trim();
    if message_id.is_empty() {
        return Err("messageId is required".to_string());
    }
    conn.query_row(
        "SELECT rowid, session_id, role FROM assistant_messages WHERE id = ?1",
        params![message_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .map_err(|e| format!("Assistant message not found {message_id}: {e}"))
}

pub fn delete_assistant_messages_after(message_id: &str, include_self: bool) -> Result<(), String> {
    init_db()?;
    let mut conn = connect()?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("Failed to start assistant history transaction: {e}"))?;
    let (rowid, session_id, _) = assistant_message_position(&tx, message_id)?;
    let comparator = if include_self { ">=" } else { ">" };
    let sql =
        format!("DELETE FROM assistant_messages WHERE session_id = ?1 AND rowid {comparator} ?2");
    tx.execute(&sql, params![session_id.as_str(), rowid])
        .map_err(|e| format!("Failed to delete assistant messages: {e}"))?;
    clear_assistant_agent_session_with_conn(&tx, &session_id)?;
    tx.execute(
        "UPDATE assistant_sessions SET updated_at = ?1 WHERE id = ?2",
        params![now_iso(), session_id.as_str()],
    )
    .map_err(|e| format!("Failed to touch assistant session: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Failed to commit assistant history delete: {e}"))
}

pub fn update_assistant_user_message_and_truncate(
    message_id: &str,
    content: &str,
) -> Result<AssistantMessage, String> {
    init_db()?;
    let content = content.trim();
    if content.is_empty() {
        return Err("assistant message content is required".to_string());
    }
    let mut conn = connect()?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("Failed to start assistant history transaction: {e}"))?;
    let (rowid, session_id, role) = assistant_message_position(&tx, message_id)?;
    if role != "user" {
        return Err("only user assistant messages can be edited".to_string());
    }
    tx.execute(
        "UPDATE assistant_messages SET content = ?1 WHERE id = ?2",
        params![content, message_id.trim()],
    )
    .map_err(|e| format!("Failed to update assistant user message: {e}"))?;
    tx.execute(
        "DELETE FROM assistant_messages WHERE session_id = ?1 AND rowid > ?2",
        params![session_id.as_str(), rowid],
    )
    .map_err(|e| format!("Failed to truncate assistant history: {e}"))?;
    clear_assistant_agent_session_with_conn(&tx, &session_id)?;
    tx.execute(
        "UPDATE assistant_sessions SET updated_at = ?1 WHERE id = ?2",
        params![now_iso(), session_id.as_str()],
    )
    .map_err(|e| format!("Failed to touch assistant session: {e}"))?;
    let message = tx
        .query_row(
            r#"
            SELECT id, session_id, role, content, scope, workspace_folder, provider, model, sources_json, created_at
            FROM assistant_messages
            WHERE id = ?1
            "#,
            params![message_id.trim()],
            row_to_assistant_message,
        )
        .map_err(|e| format!("Failed to reload assistant user message: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Failed to commit assistant history update: {e}"))?;
    Ok(message)
}

pub fn recent_assistant_final_messages(
    session_id: &str,
    limit: usize,
) -> Result<Vec<AssistantMessage>, String> {
    init_db()?;
    if session_id.trim().is_empty() {
        return Err("sessionId is required".to_string());
    }
    let conn = connect()?;
    let limit = (limit.max(1).min(40)) as i64;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, session_id, role, content, scope, workspace_folder, provider, model, sources_json, created_at
            FROM (
              SELECT id, session_id, role, content, scope, workspace_folder, provider, model, sources_json, created_at
              FROM assistant_messages
              WHERE session_id = ?1 AND role IN ('user', 'assistant')
              ORDER BY created_at DESC
              LIMIT ?2
            )
            ORDER BY created_at ASC
            "#,
        )
        .map_err(|e| format!("Failed to query assistant history: {e}"))?;
    let rows = stmt
        .query_map(params![session_id.trim(), limit], row_to_assistant_message)
        .map_err(|e| format!("Failed to read assistant history: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode assistant history: {e}"))
}

pub fn recent_assistant_final_messages_before(
    session_id: &str,
    message_id: &str,
    limit: usize,
) -> Result<Vec<AssistantMessage>, String> {
    init_db()?;
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err("sessionId is required".to_string());
    }
    let conn = connect()?;
    let (rowid, message_session_id, _) = assistant_message_position(&conn, message_id)?;
    if message_session_id != session_id {
        return Err("assistant message does not belong to session".to_string());
    }
    let limit = (limit.max(1).min(40)) as i64;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, session_id, role, content, scope, workspace_folder, provider, model, sources_json, created_at
            FROM (
              SELECT id, session_id, role, content, scope, workspace_folder, provider, model, sources_json, created_at, rowid
              FROM assistant_messages
              WHERE session_id = ?1 AND rowid < ?2 AND role IN ('user', 'assistant')
              ORDER BY rowid DESC
              LIMIT ?3
            )
            ORDER BY rowid ASC
            "#,
        )
        .map_err(|e| format!("Failed to query assistant history: {e}"))?;
    let rows = stmt
        .query_map(params![session_id, rowid, limit], row_to_assistant_message)
        .map_err(|e| format!("Failed to read assistant history: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode assistant history: {e}"))
}

pub fn get_assistant_message(message_id: &str) -> Result<AssistantMessage, String> {
    init_db()?;
    let conn = connect()?;
    conn.query_row(
        r#"
        SELECT id, session_id, role, content, scope, workspace_folder, provider, model, sources_json, created_at
        FROM assistant_messages
        WHERE id = ?1
        "#,
        params![message_id.trim()],
        row_to_assistant_message,
    )
    .map_err(|e| format!("Assistant message not found {message_id}: {e}"))
}

fn row_to_assistant_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssistantMessage> {
    let sources_json: String = row.get(8)?;
    let sources = serde_json::from_str::<Vec<AssistantSource>>(&sources_json).unwrap_or_default();
    Ok(AssistantMessage {
        id: row.get(0)?,
        session_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        scope: row.get(4)?,
        workspace_folder: row.get(5)?,
        provider: row.get(6)?,
        model: row.get(7)?,
        sources,
        created_at: row.get(9)?,
    })
}
