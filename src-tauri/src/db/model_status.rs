use rusqlite::{params, OptionalExtension};

use crate::types::ModelStatus;

use super::schema::init_db;
use super::{connect, now_iso};

pub fn get_model_status(repo: &str) -> Result<ModelStatus, String> {
    init_db()?;
    let conn = connect()?;
    let status = conn
        .query_row(
            "SELECT repo, status, path, message, updated_at FROM model_assets WHERE repo = ?1",
            params![repo],
            |row| {
                Ok(ModelStatus {
                    repo: row.get(0)?,
                    status: row.get(1)?,
                    path: row.get(2)?,
                    message: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| format!("Failed to read model status: {e}"))?;
    Ok(status.unwrap_or_else(|| ModelStatus {
        repo: repo.to_string(),
        status: "not_downloaded".to_string(),
        path: None,
        message: None,
        updated_at: None,
    }))
}

pub fn save_model_status(status: ModelStatus) -> Result<ModelStatus, String> {
    init_db()?;
    let conn = connect()?;
    conn.execute(
        r#"
        INSERT INTO model_assets (repo, status, path, message, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(repo) DO UPDATE SET
          status = excluded.status,
          path = excluded.path,
          message = excluded.message,
          updated_at = excluded.updated_at
        "#,
        params![
            status.repo,
            status.status,
            status.path,
            status.message,
            now_iso()
        ],
    )
    .map_err(|e| format!("Failed to save model status: {e}"))?;
    get_model_status(&status.repo)
}
