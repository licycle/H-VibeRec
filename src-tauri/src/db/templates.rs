use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use crate::types::{AssistantPromptTemplate, SummaryTemplate};

use super::schema::init_db;
use super::{connect, now_iso};

pub fn list_templates() -> Result<Vec<SummaryTemplate>, String> {
    init_db()?;
    let conn = connect()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, prompt, is_builtin, created_at, updated_at FROM summary_templates ORDER BY is_builtin DESC, name ASC",
        )
        .map_err(|e| format!("Failed to query templates: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SummaryTemplate {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                prompt: row.get(3)?,
                is_builtin: row.get::<_, i64>(4)? == 1,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| format!("Failed to read templates: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode templates: {e}"))
}

pub fn get_template(id: &str) -> Result<SummaryTemplate, String> {
    let conn = connect()?;
    conn.query_row(
        "SELECT id, name, description, prompt, is_builtin, created_at, updated_at FROM summary_templates WHERE id = ?1",
        params![id],
        |row| {
            Ok(SummaryTemplate {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                prompt: row.get(3)?,
                is_builtin: row.get::<_, i64>(4)? == 1,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        },
    )
    .map_err(|e| format!("Template not found {id}: {e}"))
}

pub fn save_template(
    id: Option<String>,
    name: String,
    description: Option<String>,
    prompt: String,
) -> Result<SummaryTemplate, String> {
    init_db()?;
    let conn = connect()?;
    let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let is_builtin = conn
        .query_row(
            "SELECT is_builtin FROM summary_templates WHERE id = ?1",
            params![id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| format!("Failed to check template: {e}"))?
        .unwrap_or(0);
    if is_builtin == 1 {
        return Err("Built-in templates cannot be edited".to_string());
    }

    conn.execute(
        r#"
        INSERT INTO summary_templates
          (id, name, description, prompt, is_builtin, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)
        ON CONFLICT(id) DO UPDATE SET
          name = excluded.name,
          description = excluded.description,
          prompt = excluded.prompt,
          updated_at = excluded.updated_at
        "#,
        params![id.as_str(), name, description, prompt, now_iso()],
    )
    .map_err(|e| format!("Failed to save template: {e}"))?;
    get_template(&id)
}

pub fn delete_template(id: &str) -> Result<(), String> {
    init_db()?;
    let conn = connect()?;
    let is_builtin = conn
        .query_row(
            "SELECT is_builtin FROM summary_templates WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| format!("Failed to check template: {e}"))?
        .unwrap_or(0);
    if is_builtin == 1 {
        return Err("Built-in templates cannot be deleted".to_string());
    }
    conn.execute("DELETE FROM summary_templates WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete template: {e}"))?;
    Ok(())
}

pub fn list_assistant_prompt_templates() -> Result<Vec<AssistantPromptTemplate>, String> {
    init_db()?;
    let conn = connect()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, prompt, is_builtin, created_at, updated_at FROM assistant_prompt_templates ORDER BY is_builtin DESC, name ASC",
        )
        .map_err(|e| format!("Failed to query assistant prompt templates: {e}"))?;
    let rows = stmt
        .query_map([], row_to_assistant_prompt_template)
        .map_err(|e| format!("Failed to read assistant prompt templates: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to decode assistant prompt templates: {e}"))
}

pub fn get_assistant_prompt_template(id: &str) -> Result<AssistantPromptTemplate, String> {
    let conn = connect()?;
    conn.query_row(
        "SELECT id, name, description, prompt, is_builtin, created_at, updated_at FROM assistant_prompt_templates WHERE id = ?1",
        params![id.trim()],
        row_to_assistant_prompt_template,
    )
    .map_err(|e| format!("Assistant prompt template not found {id}: {e}"))
}

fn row_to_assistant_prompt_template(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AssistantPromptTemplate> {
    Ok(AssistantPromptTemplate {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        prompt: row.get(3)?,
        is_builtin: row.get::<_, i64>(4)? == 1,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn save_assistant_prompt_template(
    id: Option<String>,
    name: String,
    description: Option<String>,
    prompt: String,
) -> Result<AssistantPromptTemplate, String> {
    init_db()?;
    let name = name.trim();
    let prompt = prompt.trim();
    if name.is_empty() {
        return Err("assistant prompt template name is required".to_string());
    }
    if prompt.is_empty() {
        return Err("assistant prompt template prompt is required".to_string());
    }
    let conn = connect()?;
    let id = id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let is_builtin = conn
        .query_row(
            "SELECT is_builtin FROM assistant_prompt_templates WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| format!("Failed to check assistant prompt template: {e}"))?
        .unwrap_or(0);
    if is_builtin == 1 {
        return Err("Built-in assistant prompt templates cannot be edited".to_string());
    }

    conn.execute(
        r#"
        INSERT INTO assistant_prompt_templates
          (id, name, description, prompt, is_builtin, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)
        ON CONFLICT(id) DO UPDATE SET
          name = excluded.name,
          description = excluded.description,
          prompt = excluded.prompt,
          updated_at = excluded.updated_at
        "#,
        params![id, name, description, prompt, now_iso()],
    )
    .map_err(|e| format!("Failed to save assistant prompt template: {e}"))?;
    get_assistant_prompt_template(&id)
}

pub fn delete_assistant_prompt_template(id: &str) -> Result<(), String> {
    init_db()?;
    let conn = connect()?;
    let is_builtin = conn
        .query_row(
            "SELECT is_builtin FROM assistant_prompt_templates WHERE id = ?1",
            params![id.trim()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| format!("Failed to check assistant prompt template: {e}"))?
        .unwrap_or(0);
    if is_builtin == 1 {
        return Err("Built-in assistant prompt templates cannot be deleted".to_string());
    }
    conn.execute(
        "DELETE FROM assistant_prompt_templates WHERE id = ?1",
        params![id.trim()],
    )
    .map_err(|e| format!("Failed to delete assistant prompt template: {e}"))?;
    Ok(())
}
