use rusqlite::{params, Connection};

use super::{
    connect, now_iso, DEFAULT_ASR_MODEL_REPO, DEFAULT_ASR_MODEL_SOURCE, DEFAULT_ASSISTANT_PROMPT,
    DEFAULT_MEETING_PROMPT, DEFAULT_VOICE_INPUT_REFINEMENT_PROMPT, SCHEMA_VERSION,
};

pub fn init_db() -> Result<(), String> {
    let conn = connect()?;
    reset_schema_if_needed(&conn)?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS recordings (
          id TEXT PRIMARY KEY,
          title TEXT NOT NULL,
          original_audio_path TEXT NOT NULL,
          normalized_audio_path TEXT,
          duration_ms INTEGER,
          file_size_bytes INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS transcription_jobs (
          id TEXT PRIMARY KEY,
          recording_id TEXT NOT NULL,
          status TEXT NOT NULL,
          engine TEXT NOT NULL,
          model_path TEXT,
          workspace_folder TEXT,
          progress INTEGER NOT NULL DEFAULT 0,
          queued_at TEXT,
          created_at TEXT NOT NULL,
          started_at TEXT,
          finished_at TEXT,
          updated_at TEXT,
          cancelled_at TEXT,
          retry_count INTEGER NOT NULL DEFAULT 0,
          output_transcript_id TEXT,
          metadata TEXT,
          error_code TEXT,
          error_message TEXT,
          FOREIGN KEY (recording_id) REFERENCES recordings(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS transcripts (
          id TEXT PRIMARY KEY,
          recording_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          text TEXT NOT NULL,
          result_json TEXT NOT NULL,
          language TEXT,
          confidence REAL,
          duration_ms INTEGER,
          rtf REAL,
          created_at TEXT NOT NULL,
          FOREIGN KEY (recording_id) REFERENCES recordings(id) ON DELETE CASCADE,
          FOREIGN KEY (job_id) REFERENCES transcription_jobs(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS summary_templates (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          description TEXT,
          prompt TEXT NOT NULL,
          is_builtin INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS summary_jobs (
          id TEXT PRIMARY KEY,
          transcript_id TEXT,
          template_id TEXT NOT NULL,
          status TEXT NOT NULL,
          provider TEXT NOT NULL,
          model TEXT NOT NULL,
          workspace_folder TEXT,
          progress INTEGER NOT NULL DEFAULT 0,
          queued_at TEXT,
          created_at TEXT NOT NULL,
          started_at TEXT,
          finished_at TEXT,
          updated_at TEXT,
          cancelled_at TEXT,
          retry_count INTEGER NOT NULL DEFAULT 0,
          summary_scope TEXT NOT NULL DEFAULT 'single_transcript',
          output_summary_id TEXT,
          metadata TEXT,
          error_code TEXT,
          error_message TEXT,
          FOREIGN KEY (transcript_id) REFERENCES transcripts(id) ON DELETE CASCADE,
          FOREIGN KEY (template_id) REFERENCES summary_templates(id)
        );

        CREATE TABLE IF NOT EXISTS summaries (
          id TEXT PRIMARY KEY,
          transcript_id TEXT,
          job_id TEXT NOT NULL,
          template_id TEXT NOT NULL,
          title TEXT,
          content TEXT NOT NULL,
          result_json TEXT,
          provider TEXT NOT NULL,
          model TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY (transcript_id) REFERENCES transcripts(id) ON DELETE CASCADE,
          FOREIGN KEY (job_id) REFERENCES summary_jobs(id) ON DELETE CASCADE,
          FOREIGN KEY (template_id) REFERENCES summary_templates(id)
        );

        CREATE TABLE IF NOT EXISTS model_assets (
          repo TEXT PRIMARY KEY,
          status TEXT NOT NULL,
          path TEXT,
          message TEXT,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS app_settings (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS assistant_sessions (
          id TEXT PRIMARY KEY,
          title TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS assistant_agent_sessions (
          session_id TEXT PRIMARY KEY,
          created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
          updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
          FOREIGN KEY (session_id) REFERENCES assistant_sessions(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS assistant_agent_items (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          session_id TEXT NOT NULL,
          message_data TEXT NOT NULL,
          created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
          FOREIGN KEY (session_id) REFERENCES assistant_agent_sessions(session_id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_assistant_agent_items_session_id
          ON assistant_agent_items(session_id, id);

        CREATE TABLE IF NOT EXISTS assistant_prompt_templates (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          description TEXT,
          prompt TEXT NOT NULL,
          is_builtin INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS assistant_messages (
          id TEXT PRIMARY KEY,
          session_id TEXT NOT NULL,
          role TEXT NOT NULL CHECK(role IN ('user', 'assistant')),
          content TEXT NOT NULL,
          scope TEXT NOT NULL CHECK(scope IN ('current', 'global')),
          workspace_folder TEXT,
          provider TEXT,
          model TEXT,
          sources_json TEXT NOT NULL DEFAULT '[]',
          created_at TEXT NOT NULL,
          FOREIGN KEY (session_id) REFERENCES assistant_sessions(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_assistant_messages_session_created
          ON assistant_messages(session_id, created_at);

        CREATE TABLE IF NOT EXISTS assistant_runs (
          request_id TEXT PRIMARY KEY,
          session_id TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed')),
          scope TEXT NOT NULL CHECK(scope IN ('current', 'global')),
          workspace_folder TEXT NOT NULL,
          question TEXT NOT NULL,
          prompt_template_id TEXT,
          web_enabled INTEGER NOT NULL DEFAULT 0,
          max_turns TEXT NOT NULL DEFAULT '16',
          current_turn INTEGER NOT NULL DEFAULT 0,
          partial_answer TEXT NOT NULL DEFAULT '',
          error_message TEXT,
          provider TEXT,
          model TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          finished_at TEXT,
          FOREIGN KEY (session_id) REFERENCES assistant_sessions(id) ON DELETE CASCADE
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_assistant_runs_running_workspace
          ON assistant_runs(workspace_folder)
          WHERE status = 'running';

        CREATE TABLE IF NOT EXISTS voice_input_daily_stats (
          day TEXT PRIMARY KEY,
          success_count INTEGER NOT NULL DEFAULT 0,
          success_chars INTEGER NOT NULL DEFAULT 0,
          last_success_at TEXT,
          last_success_chars INTEGER NOT NULL DEFAULT 0,
          updated_at TEXT NOT NULL
        );
        "#,
    )
    .map_err(|e| format!("Failed to migrate SQLite: {e}"))?;

    seed_templates(&conn)?;
    seed_assistant_prompt_templates(&conn)?;
    seed_default_settings(&conn)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|e| format!("Failed to save schema version: {e}"))?;
    Ok(())
}

fn reset_schema_if_needed(conn: &Connection) -> Result<(), String> {
    let current: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| format!("Failed to read schema version: {e}"))?;
    if current == SCHEMA_VERSION {
        return Ok(());
    }

    conn.execute_batch(
        r#"
        DROP TABLE IF EXISTS summaries;
        DROP TABLE IF EXISTS summary_jobs;
        DROP TABLE IF EXISTS summary_templates;
        DROP TABLE IF EXISTS transcripts;
        DROP TABLE IF EXISTS transcription_jobs;
        DROP TABLE IF EXISTS recordings;
        DROP TABLE IF EXISTS model_assets;
        DROP TABLE IF EXISTS app_settings;
        DROP TABLE IF EXISTS assistant_agent_items;
        DROP TABLE IF EXISTS assistant_agent_sessions;
        DROP TABLE IF EXISTS assistant_runs;
        DROP TABLE IF EXISTS assistant_messages;
        DROP TABLE IF EXISTS assistant_prompt_templates;
        DROP TABLE IF EXISTS assistant_sessions;
        DROP TABLE IF EXISTS voice_input_daily_stats;
        "#,
    )
    .map_err(|e| format!("Failed to reset local database schema: {e}"))?;
    Ok(())
}

fn seed_default_settings(conn: &Connection) -> Result<(), String> {
    let defaults = [
        ("python_path", "python3"),
        ("ffmpeg_path", "ffmpeg"),
        ("asr_model_repo", DEFAULT_ASR_MODEL_REPO),
        ("asr_model_source", DEFAULT_ASR_MODEL_SOURCE),
        ("http_proxy", ""),
        ("https_proxy", ""),
        ("all_proxy", ""),
        ("use_gpu", "true"),
        ("llm_provider", "custom"),
        ("llm_base_url", ""),
        ("llm_model", ""),
        ("llm_temperature", "0.1"),
        ("llm_max_tokens", "8192"),
        ("llm_timeout_seconds", "120"),
        ("voice_input_enabled", "false"),
        ("voice_input_hotkey", "CommandOrControl+Shift+Space"),
        ("voice_input_refinement_mode", "local"),
        (
            "voice_input_refinement_prompt",
            DEFAULT_VOICE_INPUT_REFINEMENT_PROMPT,
        ),
    ];

    for (key, value) in defaults {
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![key, value, now_iso()],
        )
        .map_err(|e| format!("Failed to seed setting {key}: {e}"))?;
    }
    Ok(())
}

fn seed_templates(conn: &Connection) -> Result<(), String> {
    let templates = [
        (
            "builtin-meeting-minutes",
            "会议纪要",
            "摘要、讨论、决策、待办和风险",
            DEFAULT_MEETING_PROMPT,
        ),
        (
            "builtin-action-items",
            "行动项提取",
            "提取和整理行动项",
            include_str!("../../assets/templates/action_items.txt"),
        ),
        (
            "builtin-decision-process",
            "决策过程梳理",
            "梳理和分析决策制定过程",
            include_str!("../../assets/templates/decision_process.txt"),
        ),
        (
            "builtin-speaker-summary",
            "发言人观点总结",
            "按发言人整理观点和立场",
            include_str!("../../assets/templates/speaker_summary.txt"),
        ),
        (
            "builtin-timeline-risk",
            "会议风险时间线分析",
            "按时间维度分析风险演化过程",
            include_str!("../../assets/templates/timeline_risk_analysis.txt"),
        ),
        (
            "builtin-topic-classification",
            "话题分类总结",
            "按话题分类整理内容",
            include_str!("../../assets/templates/topic_classification.txt"),
        ),
    ];

    for (id, name, description, prompt) in templates {
        conn.execute(
            r#"
            INSERT OR IGNORE INTO summary_templates
              (id, name, description, prompt, is_builtin, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)
            "#,
            params![id, name, description, prompt, now_iso()],
        )
        .map_err(|e| format!("Failed to seed template {id}: {e}"))?;
    }
    Ok(())
}

fn seed_assistant_prompt_templates(conn: &Connection) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO assistant_prompt_templates
          (id, name, description, prompt, is_builtin, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)
        ON CONFLICT(id) DO UPDATE SET
          name = excluded.name,
          description = excluded.description,
          prompt = excluded.prompt,
          is_builtin = 1,
          updated_at = excluded.updated_at
        "#,
        params![
            "builtin-local-notes-qa",
            "本地笔记问答",
            "严格基于当前范围本地笔记回答",
            DEFAULT_ASSISTANT_PROMPT,
            now_iso()
        ],
    )
    .map_err(|e| format!("Failed to seed assistant prompt template: {e}"))?;
    Ok(())
}
