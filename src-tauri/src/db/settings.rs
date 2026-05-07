use rusqlite::{params, Connection, OptionalExtension};

use crate::types::{AppSettings, SaveSettingsRequest};

use super::schema::init_db;
use super::{
    connect, now_iso, APP_SERVICE, DEFAULT_ASR_MODEL_REPO, DEFAULT_ASR_MODEL_SOURCE,
    DEFAULT_VOICE_INPUT_REFINEMENT_PROMPT, LEGACY_FUNASR_NANO_MODEL_REPO, LLM_KEY_ACCOUNT,
};

fn setting(conn: &Connection, key: &str, fallback: &str) -> Result<String, String> {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        params![key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| format!("Failed to read setting {key}: {e}"))
    .map(|value| value.unwrap_or_else(|| fallback.to_string()))
}

pub fn get_settings() -> Result<AppSettings, String> {
    init_db()?;
    let conn = connect()?;
    let api_key_present = has_llm_api_key();
    let raw_asr_model_repo = setting(&conn, "asr_model_repo", DEFAULT_ASR_MODEL_REPO)?;
    let raw_asr_model_source = setting(&conn, "asr_model_source", DEFAULT_ASR_MODEL_SOURCE)?;
    let legacy_default_model = raw_asr_model_repo.trim().is_empty()
        || raw_asr_model_repo.trim() == LEGACY_FUNASR_NANO_MODEL_REPO;
    Ok(AppSettings {
        python_path: setting(&conn, "python_path", "python3")?,
        ffmpeg_path: setting(&conn, "ffmpeg_path", "ffmpeg")?,
        asr_model_repo: if legacy_default_model {
            DEFAULT_ASR_MODEL_REPO.to_string()
        } else {
            raw_asr_model_repo
        },
        asr_model_source: if legacy_default_model && raw_asr_model_source == "huggingface" {
            DEFAULT_ASR_MODEL_SOURCE.to_string()
        } else {
            raw_asr_model_source
        },
        asr_model_path: setting(&conn, "asr_model_path", "")?
            .trim()
            .to_string()
            .into_option(),
        http_proxy: setting(&conn, "http_proxy", "")?
            .trim()
            .to_string()
            .into_option(),
        https_proxy: setting(&conn, "https_proxy", "")?
            .trim()
            .to_string()
            .into_option(),
        all_proxy: setting(&conn, "all_proxy", "")?
            .trim()
            .to_string()
            .into_option(),
        use_gpu: setting(&conn, "use_gpu", "true")? == "true",
        llm_provider: setting(&conn, "llm_provider", "custom")?,
        llm_base_url: setting(&conn, "llm_base_url", "")?,
        llm_model: setting(&conn, "llm_model", "")?,
        llm_temperature: setting(&conn, "llm_temperature", "0.1")?
            .parse()
            .unwrap_or(0.1),
        llm_max_tokens: setting(&conn, "llm_max_tokens", "8192")?
            .parse()
            .unwrap_or(8192),
        llm_timeout_seconds: setting(&conn, "llm_timeout_seconds", "120")?
            .parse()
            .unwrap_or(120),
        has_llm_api_key: api_key_present,
        voice_input_enabled: setting(&conn, "voice_input_enabled", "false")? == "true",
        voice_input_hotkey: setting(&conn, "voice_input_hotkey", "CommandOrControl+Shift+Space")?,
        voice_input_refinement_mode: normalized_voice_input_refinement_mode(&setting(
            &conn,
            "voice_input_refinement_mode",
            "local",
        )?),
        voice_input_refinement_prompt: setting(
            &conn,
            "voice_input_refinement_prompt",
            DEFAULT_VOICE_INPUT_REFINEMENT_PROMPT,
        )?,
    })
}

pub fn save_settings(request: SaveSettingsRequest) -> Result<AppSettings, String> {
    crate::voice_input::hotkey::parse_hotkey(&request.voice_input_hotkey)?;
    init_db()?;
    let conn = connect()?;
    let values = [
        ("python_path", request.python_path),
        ("ffmpeg_path", request.ffmpeg_path),
        ("asr_model_repo", request.asr_model_repo),
        ("asr_model_source", request.asr_model_source),
        ("asr_model_path", request.asr_model_path.unwrap_or_default()),
        ("http_proxy", request.http_proxy.unwrap_or_default()),
        ("https_proxy", request.https_proxy.unwrap_or_default()),
        ("all_proxy", request.all_proxy.unwrap_or_default()),
        ("use_gpu", request.use_gpu.to_string()),
        ("llm_provider", request.llm_provider),
        ("llm_base_url", request.llm_base_url),
        ("llm_model", request.llm_model),
        ("llm_temperature", request.llm_temperature.to_string()),
        ("llm_max_tokens", request.llm_max_tokens.to_string()),
        (
            "llm_timeout_seconds",
            request.llm_timeout_seconds.to_string(),
        ),
        (
            "voice_input_enabled",
            request.voice_input_enabled.to_string(),
        ),
        ("voice_input_hotkey", request.voice_input_hotkey),
        (
            "voice_input_refinement_mode",
            normalized_voice_input_refinement_mode(&request.voice_input_refinement_mode),
        ),
        (
            "voice_input_refinement_prompt",
            normalized_voice_input_refinement_prompt(&request.voice_input_refinement_prompt),
        ),
    ];

    for (key, value) in values {
        conn.execute(
            r#"
            INSERT INTO app_settings (key, value, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
            "#,
            params![key, value, now_iso()],
        )
        .map_err(|e| format!("Failed to save setting {key}: {e}"))?;
    }
    get_settings()
}

fn normalized_voice_input_refinement_mode(value: &str) -> String {
    match value.trim() {
        "ai_polish" => "ai_polish".to_string(),
        _ => "local".to_string(),
    }
}

fn normalized_voice_input_refinement_prompt(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        DEFAULT_VOICE_INPUT_REFINEMENT_PROMPT.to_string()
    } else {
        value.to_string()
    }
}

pub fn set_llm_api_key(api_key: String) -> Result<(), String> {
    let entry = keyring::Entry::new(APP_SERVICE, LLM_KEY_ACCOUNT)
        .map_err(|e| format!("Failed to open keychain: {e}"))?;
    if api_key.trim().is_empty() {
        let _ = entry.delete_password();
        return Ok(());
    }
    entry
        .set_password(api_key.trim())
        .map_err(|e| format!("Failed to save API key: {e}"))
}

pub fn get_llm_api_key() -> Result<String, String> {
    let entry = keyring::Entry::new(APP_SERVICE, LLM_KEY_ACCOUNT)
        .map_err(|e| format!("Failed to open keychain: {e}"))?;
    entry
        .get_password()
        .map_err(|e| format!("LLM API key is not configured: {e}"))
}

pub fn has_llm_api_key() -> bool {
    keyring::Entry::new(APP_SERVICE, LLM_KEY_ACCOUNT)
        .ok()
        .and_then(|entry| entry.get_password().ok())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

trait EmptyStringOption {
    fn into_option(self) -> Option<String>;
}

impl EmptyStringOption for String {
    fn into_option(self) -> Option<String> {
        if self.trim().is_empty() {
            None
        } else {
            Some(self)
        }
    }
}
