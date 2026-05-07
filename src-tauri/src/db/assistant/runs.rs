use rusqlite::{params, Connection, OptionalExtension};

use crate::types::{AssistantRun, AssistantWorkspaceActivity};

use super::super::schema::init_db;
use super::super::{connect, now_iso};
use super::scope::validate_assistant_scope;
use super::sessions::assistant_session_exists;

#[allow(clippy::too_many_arguments)]
pub fn create_assistant_run(
    request_id: &str,
    session_id: &str,
    scope: &str,
    workspace_folder: &str,
    question: &str,
    prompt_template_id: Option<&str>,
    web_enabled: bool,
    max_turns: &str,
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<AssistantRun, String> {
    init_db()?;
    let request_id = request_id.trim();
    let session_id = session_id.trim();
    let scope = scope.trim();
    let workspace_folder = workspace_folder.trim();
    let question = question.trim();
    let max_turns = max_turns.trim();
    if request_id.is_empty() {
        return Err("requestId is required".to_string());
    }
    if session_id.is_empty() {
        return Err("sessionId is required".to_string());
    }
    if workspace_folder.is_empty() {
        return Err("workspaceFolder is required for assistant runs".to_string());
    }
    if question.is_empty() {
        return Err("assistant run question is required".to_string());
    }
    if max_turns.is_empty() {
        return Err("assistant run maxTurns is required".to_string());
    }
    validate_assistant_scope(scope, Some(workspace_folder))?;
    let conn = connect()?;
    if !assistant_session_exists(&conn, session_id)? {
        return Err(format!("Assistant session not found: {session_id}"));
    }
    if let Some(active_request_id) = conn
        .query_row(
            "SELECT request_id FROM assistant_runs WHERE workspace_folder = ?1 AND status = 'running'",
            params![workspace_folder],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| format!("Failed to check assistant run activity: {e}"))?
    {
        return Err(format!(
            "Assistant run already active for workspace {workspace_folder}: {active_request_id}"
        ));
    }

    let now = now_iso();
    conn.execute(
        r#"
        INSERT INTO assistant_runs
          (request_id, session_id, status, scope, workspace_folder, question, prompt_template_id,
           web_enabled, max_turns, current_turn, partial_answer, error_message, provider, model,
           created_at, updated_at, finished_at)
        VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6, ?7, ?8, 0, '', NULL, ?9, ?10, ?11, ?11, NULL)
        "#,
        params![
            request_id,
            session_id,
            scope,
            workspace_folder,
            question,
            prompt_template_id
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            if web_enabled { 1_i64 } else { 0_i64 },
            max_turns,
            provider.map(str::trim).filter(|value| !value.is_empty()),
            model.map(str::trim).filter(|value| !value.is_empty()),
            now
        ],
    )
    .map_err(|e| format!("Failed to create assistant run: {e}"))?;
    get_assistant_run(request_id)
}

pub fn get_assistant_run(request_id: &str) -> Result<AssistantRun, String> {
    init_db()?;
    let request_id = request_id.trim();
    if request_id.is_empty() {
        return Err("requestId is required".to_string());
    }
    let conn = connect()?;
    assistant_run_with_conn(&conn, request_id)
}

fn assistant_run_with_conn(conn: &Connection, request_id: &str) -> Result<AssistantRun, String> {
    conn.query_row(
        r#"
        SELECT request_id, session_id, status, scope, workspace_folder, question,
               prompt_template_id, web_enabled, max_turns, current_turn, partial_answer,
               error_message, provider, model, created_at, updated_at, finished_at
        FROM assistant_runs
        WHERE request_id = ?1
        "#,
        params![request_id.trim()],
        row_to_assistant_run,
    )
    .map_err(|e| format!("Assistant run not found {request_id}: {e}"))
}

fn row_to_assistant_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssistantRun> {
    let web_enabled: i64 = row.get(7)?;
    Ok(AssistantRun {
        request_id: row.get(0)?,
        session_id: row.get(1)?,
        status: row.get(2)?,
        scope: row.get(3)?,
        workspace_folder: row.get(4)?,
        question: row.get(5)?,
        prompt_template_id: row.get(6)?,
        web_enabled: web_enabled != 0,
        max_turns: row.get(8)?,
        current_turn: row.get(9)?,
        partial_answer: row.get(10)?,
        error_message: row.get(11)?,
        provider: row.get(12)?,
        model: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
        finished_at: row.get(16)?,
    })
}

pub fn append_assistant_run_delta(request_id: &str, text: &str) -> Result<AssistantRun, String> {
    init_db()?;
    let request_id = request_id.trim();
    if request_id.is_empty() {
        return Err("requestId is required".to_string());
    }
    if text.is_empty() {
        return get_assistant_run(request_id);
    }
    let conn = connect()?;
    let changed = conn
        .execute(
            r#"
            UPDATE assistant_runs
            SET partial_answer = partial_answer || ?1,
                updated_at = ?2
            WHERE request_id = ?3 AND status = 'running'
            "#,
            params![text, now_iso(), request_id],
        )
        .map_err(|e| format!("Failed to update assistant run delta: {e}"))?;
    if changed == 0 {
        return Err(format!("Assistant run is not running: {request_id}"));
    }
    assistant_run_with_conn(&conn, request_id)
}

pub fn update_assistant_run_turn(
    request_id: &str,
    current_turn: u64,
    max_turns: Option<&str>,
) -> Result<AssistantRun, String> {
    init_db()?;
    let request_id = request_id.trim();
    if request_id.is_empty() {
        return Err("requestId is required".to_string());
    }
    let conn = connect()?;
    let now = now_iso();
    let changed =
        if let Some(max_turns) = max_turns.map(str::trim).filter(|value| !value.is_empty()) {
            conn.execute(
                r#"
            UPDATE assistant_runs
            SET current_turn = ?1,
                max_turns = ?2,
                updated_at = ?3
            WHERE request_id = ?4 AND status = 'running'
            "#,
                params![current_turn as i64, max_turns, now, request_id],
            )
        } else {
            conn.execute(
                r#"
            UPDATE assistant_runs
            SET current_turn = ?1,
                updated_at = ?2
            WHERE request_id = ?3 AND status = 'running'
            "#,
                params![current_turn as i64, now, request_id],
            )
        }
        .map_err(|e| format!("Failed to update assistant run turn: {e}"))?;
    if changed == 0 {
        return Err(format!("Assistant run is not running: {request_id}"));
    }
    assistant_run_with_conn(&conn, request_id)
}

pub fn mark_assistant_run_completed(
    request_id: &str,
    answer: &str,
) -> Result<AssistantRun, String> {
    init_db()?;
    let request_id = request_id.trim();
    if request_id.is_empty() {
        return Err("requestId is required".to_string());
    }
    let conn = connect()?;
    let now = now_iso();
    let changed = conn
        .execute(
            r#"
            UPDATE assistant_runs
            SET status = 'completed',
                partial_answer = ?1,
                error_message = NULL,
                updated_at = ?2,
                finished_at = ?2
            WHERE request_id = ?3
            "#,
            params![answer.trim(), now, request_id],
        )
        .map_err(|e| format!("Failed to complete assistant run: {e}"))?;
    if changed == 0 {
        return Err(format!("Assistant run not found: {request_id}"));
    }
    assistant_run_with_conn(&conn, request_id)
}

pub fn mark_assistant_run_failed(
    request_id: &str,
    error_message: &str,
) -> Result<AssistantRun, String> {
    init_db()?;
    let request_id = request_id.trim();
    if request_id.is_empty() {
        return Err("requestId is required".to_string());
    }
    let conn = connect()?;
    let now = now_iso();
    let message = error_message.trim();
    let changed = conn
        .execute(
            r#"
            UPDATE assistant_runs
            SET status = 'failed',
                error_message = ?1,
                updated_at = ?2,
                finished_at = ?2
            WHERE request_id = ?3
            "#,
            params![
                if message.is_empty() {
                    "assistant run failed"
                } else {
                    message
                },
                now,
                request_id
            ],
        )
        .map_err(|e| format!("Failed to fail assistant run: {e}"))?;
    if changed == 0 {
        return Err(format!("Assistant run not found: {request_id}"));
    }
    assistant_run_with_conn(&conn, request_id)
}

pub fn get_assistant_workspace_activity(
    workspace_folder: &str,
) -> Result<AssistantWorkspaceActivity, String> {
    init_db()?;
    let workspace_folder = workspace_folder.trim();
    if workspace_folder.is_empty() {
        return Err("workspaceFolder is required".to_string());
    }
    let conn = connect()?;
    let active_run = conn
        .query_row(
            r#"
            SELECT request_id, session_id, status, scope, workspace_folder, question,
                   prompt_template_id, web_enabled, max_turns, current_turn, partial_answer,
                   error_message, provider, model, created_at, updated_at, finished_at
            FROM assistant_runs
            WHERE workspace_folder = ?1 AND status = 'running'
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
            params![workspace_folder],
            row_to_assistant_run,
        )
        .optional()
        .map_err(|e| format!("Failed to query active assistant run: {e}"))?;
    let latest_session_id = conn
        .query_row(
            r#"
            SELECT session_id
            FROM assistant_runs
            WHERE workspace_folder = ?1 AND status IN ('completed', 'failed')
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
            params![workspace_folder],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| format!("Failed to query latest assistant session: {e}"))?;
    Ok(AssistantWorkspaceActivity {
        active_run,
        latest_session_id,
    })
}

pub fn recover_interrupted_assistant_runs() -> Result<(), String> {
    init_db()?;
    let conn = connect()?;
    let now = now_iso();
    conn.execute(
        r#"
        UPDATE assistant_runs
        SET status = 'failed',
            error_message = COALESCE(error_message, 'assistant run interrupted by app restart'),
            updated_at = ?1,
            finished_at = ?1
        WHERE status = 'running'
        "#,
        params![now],
    )
    .map_err(|e| format!("Failed to recover interrupted assistant runs: {e}"))?;
    Ok(())
}
