use chrono::Local;
use rusqlite::{params, OptionalExtension};

use crate::types::VoiceInputStats;

use super::schema::init_db;
use super::{connect, now_iso};

pub fn record_voice_input_success(text: &str) -> Result<VoiceInputStats, String> {
    let inserted_at = now_iso();
    record_voice_input_success_at(text, &inserted_at)?;
    let day = voice_input_day_from_iso(&inserted_at)?;
    get_voice_input_stats_for_day(&day)
}

pub fn record_voice_input_success_at(text: &str, inserted_at: &str) -> Result<(), String> {
    init_db()?;
    let conn = connect()?;
    let day = voice_input_day_from_iso(inserted_at)?;
    let char_count = crate::voice_input::text::count_inserted_chars(text);
    conn.execute(
        r#"
        INSERT INTO voice_input_daily_stats
          (day, success_count, success_chars, last_success_at, last_success_chars, updated_at)
        VALUES (?1, 1, ?2, ?3, ?2, ?4)
        ON CONFLICT(day) DO UPDATE SET
          success_count = success_count + 1,
          success_chars = success_chars + excluded.success_chars,
          last_success_at = excluded.last_success_at,
          last_success_chars = excluded.last_success_chars,
          updated_at = excluded.updated_at
        "#,
        params![day, char_count, inserted_at, now_iso()],
    )
    .map_err(|e| format!("Failed to update voice input stats: {e}"))?;
    Ok(())
}

/// Records completion of one dictation/refinement job exactly once. Delivery
/// happens later (or may fail), so it must not determine whether the voice
/// input is counted. The item UUID is the idempotency key across retries and
/// repeated notification/paste actions.
pub fn record_voice_input_completion(
    item_id: &str,
    text: &str,
) -> Result<Option<VoiceInputStats>, String> {
    init_db()?;
    let completed_at = now_iso();
    let day = voice_input_day_from_iso(&completed_at)?;
    let char_count = crate::voice_input::text::count_inserted_chars(text);
    let conn = connect()?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to start voice input stats transaction: {e}"))?;
    let inserted = tx
        .execute(
            "INSERT OR IGNORE INTO voice_input_stat_events (item_id, char_count, completed_at) VALUES (?1, ?2, ?3)",
            params![item_id, char_count, completed_at],
        )
        .map_err(|e| format!("Failed to record voice input completion: {e}"))?;
    if inserted == 0 {
        tx.rollback().map_err(|e| e.to_string())?;
        return Ok(None);
    }
    tx.execute(
        r#"
        INSERT INTO voice_input_daily_stats
          (day, success_count, success_chars, last_success_at, last_success_chars, updated_at)
        VALUES (?1, 1, ?2, ?3, ?2, ?4)
        ON CONFLICT(day) DO UPDATE SET
          success_count = success_count + 1,
          success_chars = success_chars + excluded.success_chars,
          last_success_at = excluded.last_success_at,
          last_success_chars = excluded.last_success_chars,
          updated_at = excluded.updated_at
        "#,
        params![day, char_count, completed_at, now_iso()],
    )
    .map_err(|e| format!("Failed to update voice input completion stats: {e}"))?;
    tx.commit().map_err(|e| e.to_string())?;
    get_voice_input_stats().map(Some)
}

pub fn get_voice_input_stats() -> Result<VoiceInputStats, String> {
    let today = Local::now().date_naive().to_string();
    get_voice_input_stats_for_day(&today)
}

pub fn get_voice_input_stats_for_day(day: &str) -> Result<VoiceInputStats, String> {
    init_db()?;
    let conn = connect()?;
    let day = day.trim();
    let (today_success_count, today_success_chars): (i64, i64) = conn
        .query_row(
            r#"
            SELECT success_count, success_chars
            FROM voice_input_daily_stats
            WHERE day = ?1
            "#,
            params![day],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("Failed to read today's voice input stats: {e}"))?
        .unwrap_or((0, 0));

    let (total_success_count, total_success_chars): (i64, i64) = conn
        .query_row(
            r#"
            SELECT COALESCE(SUM(success_count), 0), COALESCE(SUM(success_chars), 0)
            FROM voice_input_daily_stats
            "#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("Failed to read total voice input stats: {e}"))?;

    let last: Option<(String, i64)> = conn
        .query_row(
            r#"
            SELECT last_success_at, last_success_chars
            FROM voice_input_daily_stats
            WHERE last_success_at IS NOT NULL
            ORDER BY last_success_at DESC
            LIMIT 1
            "#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("Failed to read latest voice input stats: {e}"))?;
    let (last_success_at, last_success_chars) = match last {
        Some((at, chars)) => (Some(at), chars),
        None => (None, 0),
    };

    Ok(VoiceInputStats {
        today_success_count,
        today_success_chars,
        total_success_count,
        total_success_chars,
        last_success_at,
        last_success_chars,
    })
}

fn voice_input_day_from_iso(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.len() >= 10 {
        let day = &trimmed[..10];
        if day.chars().enumerate().all(|(index, ch)| match index {
            4 | 7 => ch == '-',
            _ => ch.is_ascii_digit(),
        }) {
            return Ok(day.to_string());
        }
    }
    Err("voice input timestamp must start with YYYY-MM-DD".to_string())
}
