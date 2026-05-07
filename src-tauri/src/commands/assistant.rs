use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::db;
use crate::sidecar;
use crate::storage;
use crate::types::{
    AssistantAskResult, AssistantMessage, AssistantRun, AssistantSession, AssistantSource,
    AssistantWorkspaceActivity, LocalNote,
};

const MAX_QUESTION_CHARS: usize = 4_000;
const HISTORY_LIMIT: usize = 20;
const DEFAULT_ASSISTANT_MAX_TURNS: &str = "16";

#[derive(Debug, Deserialize)]
pub struct AskLocalNotesAgentRequest {
    #[serde(rename = "requestId")]
    pub request_id: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    pub question: String,
    pub scope: String,
    #[serde(rename = "workspaceFolder")]
    pub workspace_folder: Option<String>,
    #[serde(rename = "promptTemplateId")]
    pub prompt_template_id: Option<String>,
    #[serde(rename = "webEnabled")]
    pub web_enabled: Option<bool>,
    #[serde(rename = "maxTurns")]
    pub max_turns: Option<Value>,
    #[serde(rename = "reuseUserMessageId")]
    pub reuse_user_message_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct AssistantStreamEventPayload {
    request_id: String,
    event: String,
    text: Option<String>,
    name: Option<String>,
    sources: Option<Vec<AssistantSource>>,
    session: Option<AssistantSession>,
    run: Option<AssistantRun>,
    user_message: Option<AssistantMessage>,
    assistant_message: Option<AssistantMessage>,
    error: Option<String>,
    current_turn: Option<u64>,
    max_turns: Option<String>,
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn ask_local_notes_agent(
    app: AppHandle,
    request: AskLocalNotesAgentRequest,
) -> Result<AssistantAskResult, String> {
    let question = validate_question(&request.question)?;
    let workspace_folder = validate_scope_and_workspace(&request.scope, request.workspace_folder)?;
    let workspace_roots = allowed_note_roots(&request.scope, workspace_folder.as_deref())?;
    let max_turns = validate_max_turns(request.max_turns)?;
    let settings = db::get_settings().map_err(structured_error("LLM_CONFIG_MISSING"))?;
    validate_llm_settings(&settings)?;
    let api_key = db::get_llm_api_key().map_err(structured_error("LLM_API_KEY_MISSING"))?;

    let session = db::get_or_create_assistant_session(
        request.session_id.as_deref(),
        Some(session_title(&question)),
    )?;
    let reused_user_message = resolve_reused_user_message(
        &session,
        request.reuse_user_message_id.as_deref(),
        &question,
    )?;
    let history = if let Some(message) = reused_user_message.as_ref() {
        db::recent_assistant_final_messages_before(&session.id, &message.id, HISTORY_LIMIT)?
    } else {
        db::recent_assistant_final_messages(&session.id, HISTORY_LIMIT)?
    };
    let session_reset_to_history = reused_user_message.is_some();
    let session_db_path = db::db_path()?.to_string_lossy().to_string();
    let prompt_template = resolve_prompt_template(request.prompt_template_id.as_deref())?;
    let sidecar_request = build_sidecar_request(
        &session,
        &question,
        &request.scope,
        workspace_folder.as_deref(),
        &workspace_roots,
        &history,
        &session_db_path,
        session_reset_to_history,
        &settings,
        &api_key,
        &prompt_template.prompt,
        request.web_enabled.unwrap_or(false),
        &max_turns,
    );

    let response = sidecar::run_assistant_sidecar(&app, sidecar_request, &settings)
        .await
        .map_err(structured_error("ASSISTANT_SIDECAR_FAILED"))?;
    let result = response
        .get("result")
        .ok_or_else(|| "ASSISTANT_SIDECAR_FAILED: sidecar response missing result".to_string())?;
    let answer = result
        .get("answer")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "ASSISTANT_SIDECAR_FAILED: sidecar returned empty answer".to_string())?;
    let sources = parse_assistant_sources(result.get("sources"))?;

    let user_message = if let Some(message) = reused_user_message {
        message
    } else {
        db::insert_assistant_message(
            &session.id,
            "user",
            &question,
            &request.scope,
            workspace_folder.as_deref(),
            Some(&settings.llm_provider),
            Some(&settings.llm_model),
            &[],
        )?
    };
    let assistant_message = db::insert_assistant_message(
        &session.id,
        "assistant",
        answer,
        &request.scope,
        workspace_folder.as_deref(),
        Some(&settings.llm_provider),
        Some(&settings.llm_model),
        &sources,
    )?;
    let session = db::get_assistant_session(&session.id)?;

    Ok(AssistantAskResult {
        session,
        user_message,
        assistant_message,
    })
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn ask_local_notes_agent_stream(
    app: AppHandle,
    request: AskLocalNotesAgentRequest,
) -> Result<AssistantAskResult, String> {
    let request_id = validate_request_id(request.request_id.as_deref())?;
    let question = validate_question(&request.question)?;
    let workspace_folder = validate_scope_and_workspace(&request.scope, request.workspace_folder)?;
    let run_workspace_folder = workspace_folder
        .as_deref()
        .ok_or_else(|| "workspaceFolder is required for assistant runs".to_string())?;
    let workspace_roots = allowed_note_roots(&request.scope, workspace_folder.as_deref())?;
    let max_turns = validate_max_turns(request.max_turns)?;
    let settings = db::get_settings().map_err(structured_error("LLM_CONFIG_MISSING"))?;
    validate_llm_settings(&settings)?;
    let api_key = db::get_llm_api_key().map_err(structured_error("LLM_API_KEY_MISSING"))?;

    let session = db::get_or_create_assistant_session(
        request.session_id.as_deref(),
        Some(session_title(&question)),
    )?;
    let reused_user_message = resolve_reused_user_message(
        &session,
        request.reuse_user_message_id.as_deref(),
        &question,
    )?;
    let history = if let Some(message) = reused_user_message.as_ref() {
        db::recent_assistant_final_messages_before(&session.id, &message.id, HISTORY_LIMIT)?
    } else {
        db::recent_assistant_final_messages(&session.id, HISTORY_LIMIT)?
    };
    let session_reset_to_history = reused_user_message.is_some();
    let session_db_path = db::db_path()?.to_string_lossy().to_string();
    let prompt_template = resolve_prompt_template(request.prompt_template_id.as_deref())?;
    let run = db::create_assistant_run(
        &request_id,
        &session.id,
        &request.scope,
        run_workspace_folder,
        &question,
        Some(&prompt_template.id),
        request.web_enabled.unwrap_or(false),
        &max_turns,
        Some(&settings.llm_provider),
        Some(&settings.llm_model),
    )?;
    emit_assistant_stream_started(&app, &request_id, &session, &run);
    let mut sidecar_request = build_sidecar_request(
        &session,
        &question,
        &request.scope,
        workspace_folder.as_deref(),
        &workspace_roots,
        &history,
        &session_db_path,
        session_reset_to_history,
        &settings,
        &api_key,
        &prompt_template.prompt,
        request.web_enabled.unwrap_or(false),
        &max_turns,
    );
    sidecar_request["id"] = json!(request_id);
    sidecar_request["type"] = json!("ask_local_notes_stream");

    let emit_app = app.clone();
    let emit_request_id = request_id.clone();
    let response =
        sidecar::run_assistant_sidecar_jsonl(&app, sidecar_request, &settings, move |event| {
            let emit_app = emit_app.clone();
            let emit_request_id = emit_request_id.clone();
            async move {
                let event_name = event
                    .get("event")
                    .and_then(|value| value.as_str())
                    .unwrap_or("progress")
                    .to_string();
                let text = event
                    .get("text")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                let current_turn = event.get("current_turn").and_then(|value| value.as_u64());
                let event_max_turns = event.get("max_turns").and_then(|value| {
                    value
                        .as_str()
                        .map(ToOwned::to_owned)
                        .or_else(|| value.as_u64().map(|number| number.to_string()))
                });
                if event_name == "delta" {
                    if let Some(text) = text.as_deref() {
                        let _ = db::append_assistant_run_delta(&emit_request_id, text);
                    }
                } else if event_name == "turn" {
                    if let Some(current_turn) = current_turn {
                        let _ = db::update_assistant_run_turn(
                            &emit_request_id,
                            current_turn,
                            event_max_turns.as_deref(),
                        );
                    }
                }
                let payload = AssistantStreamEventPayload {
                    request_id: emit_request_id,
                    event: event_name,
                    text,
                    name: event
                        .get("name")
                        .and_then(|value| value.as_str())
                        .map(ToOwned::to_owned),
                    sources: None,
                    session: None,
                    run: None,
                    user_message: None,
                    assistant_message: None,
                    error: None,
                    current_turn,
                    max_turns: event_max_turns,
                };
                let _ = emit_app.emit("assistant-stream-event", payload);
            }
        })
        .await;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let failed_run = db::mark_assistant_run_failed(&request_id, &error).ok();
            emit_assistant_stream_error(&app, &request_id, &error, failed_run);
            return Err(structured_error("ASSISTANT_SIDECAR_FAILED")(error));
        }
    };
    let finalize_result = (|| -> Result<(AssistantAskResult, AssistantRun), String> {
        let result = response.get("result").ok_or_else(|| {
            "ASSISTANT_SIDECAR_FAILED: sidecar response missing result".to_string()
        })?;
        let answer = result
            .get("answer")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "ASSISTANT_SIDECAR_FAILED: sidecar returned empty answer".to_string())?;
        let sources = parse_assistant_sources(result.get("sources"))?;

        let user_message = if let Some(message) = reused_user_message {
            message
        } else {
            db::insert_assistant_message(
                &session.id,
                "user",
                &question,
                &request.scope,
                workspace_folder.as_deref(),
                Some(&settings.llm_provider),
                Some(&settings.llm_model),
                &[],
            )?
        };
        let assistant_message = db::insert_assistant_message(
            &session.id,
            "assistant",
            answer,
            &request.scope,
            workspace_folder.as_deref(),
            Some(&settings.llm_provider),
            Some(&settings.llm_model),
            &sources,
        )?;
        let completed_run = db::mark_assistant_run_completed(&request_id, answer)?;
        let session = db::get_assistant_session(&session.id)?;
        Ok((
            AssistantAskResult {
                session,
                user_message,
                assistant_message,
            },
            completed_run,
        ))
    })();

    match finalize_result {
        Ok((ask_result, completed_run)) => {
            emit_assistant_stream_done(&app, &request_id, &ask_result, Some(completed_run));
            Ok(ask_result)
        }
        Err(error) => {
            let failed_run = db::mark_assistant_run_failed(&request_id, &error).ok();
            emit_assistant_stream_error(&app, &request_id, &error, failed_run);
            Err(error)
        }
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_assistant_workspace_activity(
    workspaceFolder: String,
) -> Result<AssistantWorkspaceActivity, String> {
    let workspace_folder = workspaceFolder.trim();
    if workspace_folder.is_empty() {
        return Err("workspaceFolder is required".to_string());
    }
    let _ = storage::strict_workspace_dir_path(workspace_folder)?;
    db::get_assistant_workspace_activity(workspace_folder)
}

#[tauri::command]
pub async fn list_assistant_sessions() -> Result<Vec<AssistantSession>, String> {
    db::list_assistant_sessions()
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn delete_assistant_session(sessionId: String) -> Result<(), String> {
    db::delete_assistant_session(&sessionId)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn list_assistant_messages(sessionId: String) -> Result<Vec<AssistantMessage>, String> {
    db::list_assistant_messages(&sessionId)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn delete_assistant_messages_after(
    messageId: String,
    includeSelf: bool,
) -> Result<(), String> {
    db::delete_assistant_messages_after(&messageId, includeSelf)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn update_assistant_user_message_and_truncate(
    messageId: String,
    content: String,
) -> Result<AssistantMessage, String> {
    db::update_assistant_user_message_and_truncate(&messageId, &content)
}

#[tauri::command]
pub async fn list_assistant_prompt_templates(
) -> Result<Vec<crate::types::AssistantPromptTemplate>, String> {
    db::list_assistant_prompt_templates()
}

#[tauri::command]
pub async fn save_assistant_prompt_template(
    id: Option<String>,
    name: String,
    description: Option<String>,
    prompt: String,
) -> Result<crate::types::AssistantPromptTemplate, String> {
    db::save_assistant_prompt_template(id, name, description, prompt)
}

#[tauri::command]
pub async fn delete_assistant_prompt_template(id: String) -> Result<(), String> {
    db::delete_assistant_prompt_template(&id)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_assistant_answer_note(
    workspaceFolder: String,
    question: String,
    answer: String,
    scope: String,
    sources: Vec<AssistantSource>,
    createdAt: Option<String>,
) -> Result<LocalNote, String> {
    validate_scope_and_workspace(&scope, Some(workspaceFolder.clone()))?;
    validate_existing_workspace(&workspaceFolder)?;
    if question.trim().is_empty() || answer.trim().is_empty() {
        return Err("question and answer are required".to_string());
    }
    let title = format!("AI问答 - {}", compact_title(&question, 36));
    let content = answer_note_markdown(&question, &answer, &scope, &sources, createdAt.as_deref());
    crate::files::save_workspace_note(
        workspaceFolder,
        LocalNote {
            id: Uuid::new_v4().to_string(),
            title,
            content,
            created: db::now_iso(),
            updated: db::now_iso(),
        },
    )
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_assistant_session_note(
    sessionId: String,
    workspaceFolder: String,
) -> Result<LocalNote, String> {
    validate_existing_workspace(&workspaceFolder)?;
    let session = db::get_assistant_session(&sessionId)?;
    let messages = db::list_assistant_messages(&sessionId)?;
    if messages.is_empty() {
        return Err("assistant session has no messages".to_string());
    }
    let title = format!("AI问答会话 - {}", compact_title(&session.title, 36));
    let content = session_note_markdown(&session, &messages);
    crate::files::save_workspace_note(
        workspaceFolder,
        LocalNote {
            id: Uuid::new_v4().to_string(),
            title,
            content,
            created: db::now_iso(),
            updated: db::now_iso(),
        },
    )
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn open_external_url(url: String) -> Result<(), String> {
    let url = validate_external_url(&url)?;
    open_url_with_system(&url)
}

fn validate_question(question: &str) -> Result<String, String> {
    let question = question.trim();
    if question.is_empty() {
        return Err("question is required".to_string());
    }
    if question.chars().count() > MAX_QUESTION_CHARS {
        return Err(format!(
            "question is too long; maximum is {MAX_QUESTION_CHARS} characters"
        ));
    }
    Ok(question.to_string())
}

fn validate_request_id(request_id: Option<&str>) -> Result<String, String> {
    let request_id = request_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "requestId is required".to_string())?;
    if request_id.len() > 120
        || !request_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err("requestId is invalid".to_string());
    }
    Ok(request_id.to_string())
}

fn validate_max_turns(max_turns: Option<Value>) -> Result<String, String> {
    let Some(value) = max_turns else {
        return Ok(DEFAULT_ASSISTANT_MAX_TURNS.to_string());
    };
    let raw = match value {
        Value::Number(number) => number.to_string(),
        Value::String(value) => value,
        _ => return Err("maxTurns must be a positive integer".to_string()),
    };
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || !trimmed.chars().all(|ch| ch.is_ascii_digit())
        || trimmed.chars().all(|ch| ch == '0')
    {
        return Err("maxTurns must be a positive integer".to_string());
    }
    Ok(trimmed.trim_start_matches('0').to_string())
}

fn resolve_reused_user_message(
    session: &AssistantSession,
    message_id: Option<&str>,
    question: &str,
) -> Result<Option<AssistantMessage>, String> {
    let Some(message_id) = message_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let message = db::get_assistant_message(message_id)?;
    if message.session_id != session.id {
        return Err("reuseUserMessageId does not belong to session".to_string());
    }
    if message.role != "user" {
        return Err("reuseUserMessageId must reference a user message".to_string());
    }
    if message.content.trim() != question.trim() {
        return Err("reuseUserMessageId content does not match question".to_string());
    }
    Ok(Some(message))
}

fn validate_scope_and_workspace(
    scope: &str,
    workspace_folder: Option<String>,
) -> Result<Option<String>, String> {
    let workspace_folder = workspace_folder
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    db::validate_assistant_scope(scope, workspace_folder.as_deref())?;
    if let Some(folder) = workspace_folder.as_deref() {
        let _ = storage::strict_workspace_dir_path(folder)?;
    }
    Ok(workspace_folder)
}

fn validate_existing_workspace(workspace_folder: &str) -> Result<(), String> {
    let workspace_dir = storage::strict_workspace_dir_path(workspace_folder)?;
    if !workspace_dir.is_dir() {
        return Err(format!("workspace not found: {workspace_folder}"));
    }
    Ok(())
}

fn allowed_note_roots(scope: &str, workspace_folder: Option<&str>) -> Result<Vec<Value>, String> {
    if scope == "current" {
        let folder = workspace_folder
            .ok_or_else(|| "workspaceFolder is required for current scope".to_string())?;
        let workspace_dir = storage::strict_workspace_dir_path(folder)?;
        validate_workspace_exists(&workspace_dir, folder)?;
        return Ok(vec![json!({
            "workspace_folder": folder,
            "notes_dir": workspace_dir.join("notes").to_string_lossy().to_string()
        })]);
    }

    let workspaces_dir = storage::get_workspaces_dir()?;
    let mut roots = Vec::new();
    for entry in std::fs::read_dir(&workspaces_dir)
        .map_err(|e| format!("Failed to read workspaces directory: {e}"))?
        .flatten()
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(folder) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let strict = storage::strict_workspace_dir_path(folder)?;
        if strict != path {
            continue;
        }
        roots.push(json!({
            "workspace_folder": folder,
            "notes_dir": path.join("notes").to_string_lossy().to_string()
        }));
    }
    roots.sort_by(|a, b| {
        a.get("workspace_folder")
            .and_then(|value| value.as_str())
            .cmp(&b.get("workspace_folder").and_then(|value| value.as_str()))
    });
    Ok(roots)
}

fn validate_workspace_exists(path: &Path, folder: &str) -> Result<(), String> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(format!("workspace not found: {folder}"))
    }
}

fn validate_llm_settings(settings: &crate::types::AppSettings) -> Result<(), String> {
    if settings.llm_base_url.trim().is_empty() {
        return Err("LLM_CONFIG_MISSING: llm_base_url is not configured".to_string());
    }
    if settings.llm_model.trim().is_empty() {
        return Err("LLM_CONFIG_MISSING: llm_model is not configured".to_string());
    }
    Ok(())
}

fn resolve_prompt_template(
    prompt_template_id: Option<&str>,
) -> Result<crate::types::AssistantPromptTemplate, String> {
    let id = prompt_template_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("builtin-local-notes-qa");
    let template = db::get_assistant_prompt_template(id)?;
    if template.prompt.trim().is_empty() {
        return Err("assistant prompt template prompt is empty".to_string());
    }
    Ok(template)
}

fn build_sidecar_request(
    session: &AssistantSession,
    question: &str,
    scope: &str,
    workspace_folder: Option<&str>,
    workspace_roots: &[Value],
    history: &[AssistantMessage],
    session_db_path: &str,
    session_reset_to_history: bool,
    settings: &crate::types::AppSettings,
    api_key: &str,
    prompt: &str,
    web_enabled: bool,
    max_turns: &str,
) -> Value {
    json!({
        "id": Uuid::new_v4().to_string(),
        "type": "ask_local_notes",
        "payload": {
            "session_id": session.id,
            "session_db_path": session_db_path,
            "session_reset_to_history": session_reset_to_history,
            "question": question,
            "scope": scope,
            "workspace_folder": workspace_folder,
            "workspace_roots": workspace_roots,
            "history": history.iter().map(|message| json!({
                "role": message.role,
                "content": message.content
            })).collect::<Vec<_>>(),
            "llm": {
                "provider": settings.llm_provider,
                "base_url": chat_completions_base_url(&settings.llm_provider, &settings.llm_base_url),
                "model": settings.llm_model,
                "api_key": api_key,
                "temperature": settings.llm_temperature,
                "max_tokens": settings.llm_max_tokens,
                "timeout_seconds": settings.llm_timeout_seconds
            },
            "prompt": prompt,
            "web_enabled": web_enabled,
            "max_turns": max_turns
        }
    })
}

fn chat_completions_base_url(_provider: &str, base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if let Some(stripped) = base.strip_suffix("/chat/completions") {
        return stripped.trim_end_matches("/v1").to_string() + "/v1";
    }
    if base.ends_with("/v1") {
        return base.to_string();
    }
    format!("{base}/v1")
}

fn parse_assistant_sources(value: Option<&Value>) -> Result<Vec<AssistantSource>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let sources: Vec<AssistantSource> = serde_json::from_value(value.clone())
        .map_err(|e| format!("ASSISTANT_SIDECAR_FAILED: invalid sources payload: {e}"))?;
    for source in &sources {
        let source_type = source.source_type.as_deref().unwrap_or("note").trim();
        match source_type {
            "web" => {
                let url = source
                    .url
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(source.id.trim());
                validate_external_url(url).map_err(|_| {
                    "ASSISTANT_SIDECAR_FAILED: invalid web source entry".to_string()
                })?;
                if source.title.trim().is_empty() {
                    return Err("ASSISTANT_SIDECAR_FAILED: invalid web source entry".to_string());
                }
            }
            "note" | "" => {
                if source.id.trim().is_empty()
                    || source.note_id.trim().is_empty()
                    || source.title.trim().is_empty()
                {
                    return Err("ASSISTANT_SIDECAR_FAILED: invalid source entry".to_string());
                }
                if let (Some(start), Some(end)) = (source.start_line, source.end_line) {
                    if start == 0 || end < start {
                        return Err(
                            "ASSISTANT_SIDECAR_FAILED: invalid source line range".to_string()
                        );
                    }
                }
            }
            _ => return Err("ASSISTANT_SIDECAR_FAILED: invalid source type".to_string()),
        }
    }
    Ok(sources)
}

fn validate_external_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("url is required".to_string());
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http and https URLs can be opened".to_string());
    }
    if url.contains(char::is_whitespace) {
        return Err("url is invalid".to_string());
    }
    Ok(url.to_string())
}

fn open_url_with_system(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status();
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd").args(["/C", "start", "", url]).status();
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("xdg-open").arg(url).status();

    status
        .map_err(|e| format!("Failed to open URL: {e}"))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("Failed to open URL: {status}"))
            }
        })
}

fn session_title(question: &str) -> String {
    format!("AI问答 - {}", compact_title(question, 28))
}

fn compact_title(value: &str, max_chars: usize) -> String {
    let cleaned = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    let title = if cleaned.is_empty() {
        "未命名".to_string()
    } else {
        cleaned
    };
    if title.chars().count() <= max_chars {
        return title;
    }
    format!("{}...", title.chars().take(max_chars).collect::<String>())
}

fn answer_note_markdown(
    question: &str,
    answer: &str,
    scope: &str,
    sources: &[AssistantSource],
    created_at: Option<&str>,
) -> String {
    let created_at = created_at.unwrap_or("");
    format!(
        "# AI问答\n\n## 问题\n\n{}\n\n## 回答\n\n{}\n\n## 元信息\n\n- 范围：{}\n- 时间：{}\n\n{}",
        question.trim(),
        answer.trim(),
        scope_label(scope),
        created_at,
        sources_markdown(sources)
    )
}

fn session_note_markdown(session: &AssistantSession, messages: &[AssistantMessage]) -> String {
    let mut output = format!(
        "# {}\n\n- 创建时间：{}\n- 更新时间：{}\n\n",
        session.title, session.created_at, session.updated_at
    );
    for message in messages {
        if message.role == "user" {
            output.push_str("## 问题\n\n");
            output.push_str(message.content.trim());
            output.push_str("\n\n");
        } else {
            output.push_str("## 回答\n\n");
            output.push_str(message.content.trim());
            output.push_str("\n\n");
            output.push_str("### 元信息\n\n");
            output.push_str(&format!(
                "- 范围：{}\n- 时间：{}\n\n",
                scope_label(&message.scope),
                message.created_at
            ));
            output.push_str(&sources_markdown(&message.sources));
            output.push_str("\n\n");
        }
    }
    output.trim_end().to_string()
}

fn sources_markdown(sources: &[AssistantSource]) -> String {
    if sources.is_empty() {
        return "## 引用来源\n\n未找到可引用来源。".to_string();
    }
    let rows = sources
        .iter()
        .map(|source| {
            if source.source_type.as_deref() == Some("web") {
                let url = source.url.as_deref().unwrap_or(source.id.as_str()).trim();
                return format!("- [{}]({})", source.title.trim(), url);
            }
            let workspace = source.workspace_folder.as_deref().unwrap_or("-");
            let line_suffix = match (source.start_line, source.end_line) {
                (Some(start), Some(end)) if start == end => format!(" / L{start}"),
                (Some(start), Some(end)) => format!(" / L{start}-L{end}"),
                _ => String::new(),
            };
            format!(
                "- {}（{} / {}{}）",
                source.title.trim(),
                workspace,
                source.note_id.trim(),
                line_suffix
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("## 引用来源\n\n{rows}")
}

fn scope_label(scope: &str) -> &'static str {
    match scope {
        "global" => "全部空间",
        _ => "当前空间",
    }
}

fn emit_assistant_stream_started(
    app: &AppHandle,
    request_id: &str,
    session: &AssistantSession,
    run: &AssistantRun,
) {
    let payload = AssistantStreamEventPayload {
        request_id: request_id.to_string(),
        event: "started".to_string(),
        text: None,
        name: None,
        sources: None,
        session: Some(session.clone()),
        run: Some(run.clone()),
        user_message: None,
        assistant_message: None,
        error: None,
        current_turn: Some(run.current_turn as u64),
        max_turns: Some(run.max_turns.clone()),
    };
    let _ = app.emit("assistant-stream-event", payload);
}

fn emit_assistant_stream_done(
    app: &AppHandle,
    request_id: &str,
    result: &AssistantAskResult,
    run: Option<AssistantRun>,
) {
    let payload = AssistantStreamEventPayload {
        request_id: request_id.to_string(),
        event: "done".to_string(),
        text: Some(result.assistant_message.content.clone()),
        name: None,
        sources: Some(result.assistant_message.sources.clone()),
        session: Some(result.session.clone()),
        run,
        user_message: Some(result.user_message.clone()),
        assistant_message: Some(result.assistant_message.clone()),
        error: None,
        current_turn: None,
        max_turns: None,
    };
    let _ = app.emit("assistant-stream-event", payload);
}

fn emit_assistant_stream_error(
    app: &AppHandle,
    request_id: &str,
    error: &str,
    run: Option<AssistantRun>,
) {
    let payload = AssistantStreamEventPayload {
        request_id: request_id.to_string(),
        event: "error".to_string(),
        text: None,
        name: None,
        sources: None,
        session: None,
        run,
        user_message: None,
        assistant_message: None,
        error: Some(error.to_string()),
        current_turn: None,
        max_turns: None,
    };
    let _ = app.emit("assistant-stream-event", payload);
}

fn structured_error(code: &'static str) -> impl FnOnce(String) -> String {
    move |message| {
        if message.starts_with(code) {
            message
        } else {
            format!("{code}: {message}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestAppData {
        _guard: std::sync::MutexGuard<'static, ()>,
        path: PathBuf,
    }

    impl TestAppData {
        fn new(name: &str) -> Self {
            let guard = support::lock_app_data();
            let path = std::env::temp_dir()
                .join(format!("voice-vibe-assistant-{name}-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create test app data dir");
            std::env::set_var("VOICE_VIBE_TEST_APP_DATA_DIR", &path);
            Self {
                _guard: guard,
                path,
            }
        }
    }

    impl Drop for TestAppData {
        fn drop(&mut self) {
            std::env::remove_var("VOICE_VIBE_TEST_APP_DATA_DIR");
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn rejects_empty_question() {
        assert!(validate_question("   ").is_err());
    }

    #[test]
    fn rejects_invalid_stream_request_id() {
        assert!(validate_request_id(Some("abc-123_DEF")).is_ok());
        assert!(validate_request_id(Some("../bad")).is_err());
        assert!(validate_request_id(None).is_err());
    }

    #[test]
    fn rejects_path_traversal_workspace_folder() {
        let _app_data = TestAppData::new("traversal");

        let result = validate_scope_and_workspace("current", Some("../outside".to_string()));

        assert!(result.is_err());
    }

    #[test]
    fn current_scope_requires_workspace_folder() {
        let result = validate_scope_and_workspace("current", None);

        assert!(result.is_err());
    }

    #[test]
    fn global_roots_are_derived_from_app_workspaces() {
        let _app_data = TestAppData::new("roots");
        let workspaces = storage::get_workspaces_dir().expect("workspaces dir");
        std::fs::create_dir_all(workspaces.join("space-a").join("notes")).expect("space a");
        std::fs::create_dir_all(workspaces.join("space-b").join("notes")).expect("space b");

        let roots = allowed_note_roots("global", None).expect("global roots");

        assert_eq!(roots.len(), 2);
        assert_eq!(
            roots[0]
                .get("workspace_folder")
                .and_then(|value| value.as_str()),
            Some("space-a")
        );
    }

    #[test]
    fn assistant_sidecar_request_carries_positive_unbounded_max_turns() {
        assert_eq!(
            validate_max_turns(Some(json!("128"))).expect("max turns"),
            "128"
        );
        assert_eq!(
            validate_max_turns(Some(json!("123456789012345678901234567890"))).expect("max turns"),
            "123456789012345678901234567890"
        );
        assert!(validate_max_turns(Some(json!("0"))).is_err());

        let settings = crate::types::AppSettings {
            python_path: "".to_string(),
            ffmpeg_path: "".to_string(),
            asr_model_repo: "".to_string(),
            asr_model_source: "".to_string(),
            asr_model_path: None,
            http_proxy: None,
            https_proxy: None,
            all_proxy: None,
            use_gpu: false,
            llm_provider: "compatible".to_string(),
            llm_base_url: "https://api.example.com/v1".to_string(),
            llm_model: "test-model".to_string(),
            llm_temperature: 0.1,
            llm_max_tokens: 2048,
            llm_timeout_seconds: 120,
            has_llm_api_key: true,
            voice_input_enabled: false,
            voice_input_hotkey: "CommandOrControl+Shift+Space".to_string(),
            voice_input_refinement_mode: "local".to_string(),
            voice_input_refinement_prompt: "{{ transcript }}".to_string(),
        };
        let session = AssistantSession {
            id: "session-1".to_string(),
            title: "Session".to_string(),
            created_at: "2026-05-05T00:00:00Z".to_string(),
            updated_at: "2026-05-05T00:00:00Z".to_string(),
        };

        let request = build_sidecar_request(
            &session,
            "问题",
            "current",
            Some("space-a"),
            &[json!({"workspace_folder": "space-a", "notes_dir": "/tmp/space-a/notes"})],
            &[],
            "/tmp/app.sqlite",
            false,
            &settings,
            "test-key",
            "请回答",
            true,
            "128",
        );

        assert_eq!(request["payload"]["max_turns"].as_str(), Some("128"));
        assert_eq!(request["payload"]["session_id"].as_str(), Some("session-1"));
        assert_eq!(
            request["payload"]["session_reset_to_history"].as_bool(),
            Some(false)
        );
        assert!(request["payload"]["session_db_path"]
            .as_str()
            .unwrap_or_default()
            .ends_with("app.sqlite"));
    }
}
