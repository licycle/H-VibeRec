use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::{connect, now_iso};
use crate::types::{PasteItem, PasteMode, PasteTarget};

pub fn pastebox_notifications_enabled() -> Result<bool, String> {
    let value: Option<String> = connect()?
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'pastebox_notifications_enabled'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    value
        .map(|v| serde_json::from_str(&v).map_err(|e| e.to_string()))
        .transpose()
        .map(|v| v.unwrap_or(true))
}

pub fn set_pastebox_notifications_enabled(enabled: bool) -> Result<(), String> {
    connect()?.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES ('pastebox_notifications_enabled', ?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![enabled.to_string(), now_iso()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

// Additive migration: never bump the legacy destructive schema version for this table.
pub(super) fn migrate_pastebox(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS voice_input_outbox (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT NOT NULL UNIQUE,
            raw_text TEXT NOT NULL,
            text TEXT NOT NULL,
            processing_status TEXT NOT NULL,
            delivery_status TEXT NOT NULL DEFAULT 'pending',
            mode TEXT NOT NULL,
            target_json TEXT,
            created_at TEXT NOT NULL,
            version INTEGER NOT NULL DEFAULT 0,
            actioned INTEGER NOT NULL DEFAULT 0,
            delivery_token TEXT,
            error TEXT,
            polish_error TEXT,
            notification_error TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_voice_input_outbox_status
        ON voice_input_outbox(delivery_status, seq);
        CREATE TABLE IF NOT EXISTS voice_input_stat_events (
            item_id TEXT PRIMARY KEY,
            char_count INTEGER NOT NULL,
            completed_at TEXT NOT NULL
        );
        DELETE FROM app_settings WHERE key = 'pastebox_shortcuts';",
    )
    .map_err(|e| format!("无法建立语音粘贴箱：{e}"))
}

pub fn pastebox_mode() -> Result<PasteMode, String> {
    let value: Option<String> = connect()?
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'voice_input_paste_mode'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(value
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default())
}

pub fn set_pastebox_mode(mode: PasteMode) -> Result<(), String> {
    connect()?.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES ('voice_input_paste_mode', ?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![serde_json::to_string(&mode).map_err(|e| e.to_string())?, now_iso()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn insert_paste_item(
    raw: &str,
    mode: PasteMode,
    target: Option<&PasteTarget>,
    refining: bool,
) -> Result<PasteItem, String> {
    if raw.trim().is_empty() {
        return Err("转录为空".into());
    }
    let id = Uuid::new_v4().to_string();
    connect()?.execute(
        "INSERT INTO voice_input_outbox (id, raw_text, text, processing_status, mode, target_json, created_at)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6)",
        params![id, raw, if refining { "refining" } else { "ready" },
            serde_json::to_string(&mode).map_err(|e| e.to_string())?,
            target.map(serde_json::to_string).transpose().map_err(|e| e.to_string())?, now_iso()],
    ).map_err(|e| format!("无法保存转录：{e}"))?;
    get_paste_item(&id)
}

// Allocate the public sequence/UUID when audio is submitted, before ASR. The
// same row then travels through every background stage and notification.
pub fn insert_dictation_job(id: &str, mode: PasteMode) -> Result<PasteItem, String> {
    connect()?
        .execute(
            "INSERT INTO voice_input_outbox
        (id, raw_text, text, processing_status, mode, created_at)
        VALUES (?1, '', '', 'queued', ?2, ?3)",
            params![
                id,
                serde_json::to_string(&mode).map_err(|e| e.to_string())?,
                now_iso()
            ],
        )
        .map_err(|e| e.to_string())?;
    get_paste_item(id)
}

pub fn set_dictation_target(id: &str, target: Option<&PasteTarget>) -> Result<(), String> {
    connect()?
        .execute(
            "UPDATE voice_input_outbox SET target_json = ?2 WHERE id = ?1",
            params![
                id,
                target
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_dictation_stage(id: &str, stage: &str) -> Result<(), String> {
    connect()?
        .execute(
            "UPDATE voice_input_outbox SET processing_status = ?2,
        version = version + 1 WHERE id = ?1",
            params![id, stage],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn finish_dictation_transcription(id: &str, raw: &str, refining: bool) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("转录为空".into());
    }
    connect()?
        .execute(
            "UPDATE voice_input_outbox SET raw_text = ?2, text = ?2,
        processing_status = ?3, error = NULL, version = version + 1 WHERE id = ?1",
            params![id, raw, if refining { "refining" } else { "ready" }],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn fail_dictation_job(id: &str, error: &str) -> Result<(), String> {
    connect()?
        .execute(
            "UPDATE voice_input_outbox SET processing_status = 'failed',
        error = ?2, version = version + 1 WHERE id = ?1",
            params![id, error],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn retry_dictation_job(id: &str, version: i64) -> Result<PasteItem, String> {
    let changed = connect()?
        .execute(
            "UPDATE voice_input_outbox SET processing_status = 'queued',
        error = NULL, version = version + 1 WHERE id = ?1 AND version = ?2
        AND processing_status = 'failed' AND raw_text = '' AND actioned = 0",
            params![id, version],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("任务已更新或正在处理中，请刷新后重试".into());
    }
    get_paste_item(id)
}

pub fn has_earlier_automatic_job(seq: i64) -> Result<bool, String> {
    connect()?
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM voice_input_outbox
        WHERE seq < ?1 AND mode = '\"automatic\"' AND actioned = 0
        AND delivery_status = 'pending' AND processing_status != 'failed')",
            [seq],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
}

pub fn active_dictation_count() -> Result<i64, String> {
    connect()?
        .query_row(
            "SELECT COUNT(*) FROM voice_input_outbox WHERE processing_status
        IN ('queued','preparing_model','transcribing','refining')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
}

pub fn fail_dictation_completion(id: &str, error: &str) -> Result<(), String> {
    connect()?
        .execute(
            "UPDATE voice_input_outbox SET processing_status = 'ready',
        delivery_status = CASE WHEN actioned = 0 THEN 'failed' ELSE delivery_status END,
        error = ?2, version = version + 1 WHERE id = ?1",
            params![id, error],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn finish_paste_refinement(
    id: &str,
    text: &str,
    error: Option<&str>,
) -> Result<PasteItem, String> {
    connect()?.execute(
        "UPDATE voice_input_outbox SET text = ?2, processing_status = 'ready', polish_error = ?3,
         version = version + 1 WHERE id = ?1", params![id, text, error],
    ).map_err(|e| e.to_string())?;
    get_paste_item(id)
}

const SELECT_ITEM: &str = "SELECT id, seq, raw_text, text, processing_status, delivery_status,
    mode, target_json, created_at, version, actioned, error, polish_error, notification_error FROM voice_input_outbox";

fn row_to_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<PasteItem> {
    fn decode<T: serde::de::DeserializeOwned>(column: usize, value: &str) -> rusqlite::Result<T> {
        serde_json::from_str(value).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                column,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })
    }
    Ok(PasteItem {
        id: r.get(0)?,
        seq: r.get(1)?,
        raw_text: r.get(2)?,
        text: r.get(3)?,
        processing_status: r.get(4)?,
        delivery_status: r.get(5)?,
        mode: decode(6, &r.get::<_, String>(6)?)?,
        target: r
            .get::<_, Option<String>>(7)?
            .map(|v| decode(7, &v))
            .transpose()?,
        created_at: r.get(8)?,
        version: r.get(9)?,
        actioned: r.get(10)?,
        error: r.get(11)?,
        polish_error: r.get(12)?,
        notification_error: r.get(13)?,
    })
}

pub fn get_paste_item(id: &str) -> Result<PasteItem, String> {
    connect()?
        .query_row(&format!("{SELECT_ITEM} WHERE id = ?1"), [id], row_to_item)
        .map_err(|e| format!("粘贴记录不存在或不可读取：{e}"))
}

pub fn list_paste_items(
    before_seq: Option<i64>,
    query: &str,
    pending_only: bool,
) -> Result<Vec<PasteItem>, String> {
    let conn = connect()?;
    let mut stmt = conn
        .prepare(&format!(
            "{SELECT_ITEM} WHERE (?1 IS NULL OR seq < ?1)
        AND (?2 = '' OR instr(text, ?2) > 0 OR instr(raw_text, ?2) > 0 OR CAST(seq AS TEXT) = ?2)
        AND (?3 = 0 OR delivery_status IN ('pending', 'failed', 'uncertain', 'delivering'))
        ORDER BY seq DESC LIMIT 50"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![before_seq, query.trim(), pending_only], row_to_item)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn pending_paste_count() -> Result<i64, String> {
    connect()?
        .query_row(
            "SELECT COUNT(*) FROM voice_input_outbox
        WHERE delivery_status IN ('pending', 'failed', 'uncertain', 'delivering')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
}

pub fn next_pending_paste_item() -> Result<Option<PasteItem>, String> {
    connect()?
        .query_row(
            &format!(
                "{SELECT_ITEM} WHERE delivery_status = 'pending'
        AND processing_status = 'ready' AND actioned = 0 ORDER BY seq ASC LIMIT 1"
            ),
            [],
            row_to_item,
        )
        .optional()
        .map_err(|e| e.to_string())
}

// A delivery lease survives refinement updates. Versions reject stale UI clicks;
// pending_only additionally makes notification and automatic delivery single-use.
pub fn claim_paste_item(
    id: &str,
    version: i64,
    pending_only: bool,
) -> Result<(PasteItem, String), String> {
    let token = Uuid::new_v4().to_string();
    let changed = connect()?.execute("UPDATE voice_input_outbox
        SET delivery_status = 'delivering', delivery_token = ?3, actioned = 1, error = NULL, version = version + 1
        WHERE id = ?1 AND version = ?2 AND delivery_status != 'delivering'
        AND raw_text != '' AND processing_status IN ('ready', 'refining')
        AND (?4 = 0 OR (actioned = 0 AND delivery_status = 'pending' AND processing_status = 'ready'))",
        params![id, version, token, pending_only]).map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("记录已更新或已经处理，请刷新后重试".into());
    }
    Ok((get_paste_item(id)?, token))
}

pub fn finish_paste_delivery(
    id: &str,
    token: &str,
    status: &str,
    error: Option<&str>,
) -> Result<PasteItem, String> {
    let changed = connect()?
        .execute(
            "UPDATE voice_input_outbox SET delivery_status = ?3, error = ?4,
        delivery_token = NULL, version = version + 1 WHERE id = ?1 AND delivery_token = ?2",
            params![id, token, status, error],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("投递状态已变化".into());
    }
    get_paste_item(id)
}

pub fn set_paste_notification_error(id: &str, error: Option<&str>) -> Result<(), String> {
    connect()?
        .execute(
            "UPDATE voice_input_outbox SET notification_error = ?2 WHERE id = ?1",
            params![id, error],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_paste_item(id: &str, version: i64) -> Result<(), String> {
    let changed = connect()?
        .execute(
            "DELETE FROM voice_input_outbox WHERE id = ?1 AND version = ?2
        AND delivery_status != 'delivering' AND processing_status IN ('ready', 'failed')",
            params![id, version],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("记录仍在处理中或已更新，请稍后重试".into());
    }
    Ok(())
}

/// Remove all completed/history rows in one transaction. Active recording,
/// transcription and refinement rows stay intact so clearing the panel cannot
/// cancel work that is still running. Returns IDs whose notification/audio
/// artifacts should be cleaned by the pastebox service.
pub fn clear_paste_history() -> Result<Vec<String>, String> {
    let mut conn = connect()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut stmt = tx
        .prepare(
            "SELECT id FROM voice_input_outbox
                  WHERE delivery_status != 'delivering'
                    AND processing_status IN ('ready', 'failed')",
        )
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    tx.execute(
        "DELETE FROM voice_input_outbox
         WHERE delivery_status != 'delivering'
           AND processing_status IN ('ready', 'failed')",
        [],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(ids)
}

pub fn recover_pastebox() -> Result<(), String> {
    connect()?.execute_batch("UPDATE voice_input_outbox SET processing_status = 'failed',
        error = '应用退出中断了转录，录音已保留，可以重试', version = version + 1
        WHERE processing_status IN ('queued', 'preparing_model', 'transcribing');
        UPDATE voice_input_outbox SET delivery_status = 'failed',
        error = '应用已重启，原位置需要重新确认，请手动选择位置', version = version + 1
        WHERE mode = '\"automatic\"' AND processing_status IN ('refining', 'ready')
        AND delivery_status = 'pending';
        UPDATE voice_input_outbox SET processing_status = 'ready',
        polish_error = '应用重启中断了润色，已保留原文', version = version + 1 WHERE processing_status = 'refining';
        UPDATE voice_input_outbox SET delivery_status = 'uncertain', delivery_token = NULL,
        error = '应用在粘贴期间退出，请检查目标内容后再操作', version = version + 1 WHERE delivery_status = 'delivering';")
        .map_err(|e| e.to_string())
}
