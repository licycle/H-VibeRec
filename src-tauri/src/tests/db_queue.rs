use std::path::PathBuf;

use rusqlite::params;
use uuid::Uuid;

use crate::db;
use crate::tests::support;
use crate::types::{LocalQueueJob, SummaryTemplate};

struct TestAppData {
    _guard: std::sync::MutexGuard<'static, ()>,
    path: PathBuf,
}

impl TestAppData {
    fn new(name: &str) -> Self {
        let guard = support::lock_app_data();
        let path = std::env::temp_dir().join(format!("voice-vibe-local-{name}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("create test app data dir");
        std::env::set_var("VOICE_VIBE_TEST_APP_DATA_DIR", &path);
        db::init_db().expect("initialize test DB");
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

fn write_test_recording(app_data: &TestAppData, name: &str) -> PathBuf {
    let path = app_data.path.join(name);
    std::fs::write(&path, b"test-audio").expect("write test recording");
    path
}

fn test_template() -> SummaryTemplate {
    db::save_template(
        Some("test-template".to_string()),
        "Test Template".to_string(),
        None,
        "Summarize {{ transcript }}".to_string(),
    )
    .expect("create test template")
}

fn pipeline(job: &LocalQueueJob) -> &serde_json::Value {
    job.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("pipeline"))
        .expect("job metadata pipeline")
}

fn pipeline_steps(job: &LocalQueueJob) -> &Vec<serde_json::Value> {
    pipeline(job)
        .get("steps")
        .and_then(|steps| steps.as_array())
        .expect("pipeline steps")
}

fn pipeline_step<'a>(job: &'a LocalQueueJob, name: &str) -> &'a serde_json::Value {
    pipeline_steps(job)
        .iter()
        .find(|step| step.get("name").and_then(|value| value.as_str()) == Some(name))
        .unwrap_or_else(|| panic!("pipeline step {name}"))
}

fn step_status<'a>(job: &'a LocalQueueJob, name: &str) -> &'a str {
    pipeline_step(job, name)
        .get("status")
        .and_then(|value| value.as_str())
        .expect("step status")
}

#[test]
fn recording_id_uses_workspace_folder_for_workspace_recordings() {
    let path = PathBuf::from("/tmp/app/workspaces/weekly-sync/recordings/call.wav");
    assert_eq!(db::recording_id_from_path(&path), "weekly-sync__call");
}

#[test]
fn recording_id_falls_back_to_file_stem() {
    let path = PathBuf::from("/tmp/audio/call.wav");
    assert_eq!(db::recording_id_from_path(&path), "call");
}

#[test]
fn init_db_rebuilds_when_schema_version_changes() {
    let _app_data = TestAppData::new("schema-reset");
    let conn = db::connect().expect("connect test DB");
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES ('legacy-key', 'legacy', 'now')",
        [],
    )
    .expect("insert legacy row");
    conn.pragma_update(None, "user_version", 1)
        .expect("set old user version");
    drop(conn);

    db::init_db().expect("reinitialize test DB");
    let conn = db::connect().expect("connect rebuilt DB");
    let legacy_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM app_settings WHERE key = 'legacy-key'",
            [],
            |row| row.get(0),
        )
        .expect("query legacy key");
    let current_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read schema version");

    assert_eq!(legacy_count, 0);
    let assistant_runs_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'assistant_runs'",
            [],
            |row| row.get(0),
        )
        .expect("query assistant_runs table");

    assert_eq!(legacy_count, 0);
    assert_eq!(assistant_runs_count, 1);
    assert_eq!(current_version, 9);
}

fn insert_assistant_run(
    request_id: &str,
    session_id: &str,
    workspace_folder: &str,
) -> crate::types::AssistantRun {
    db::create_assistant_run(
        request_id,
        session_id,
        "current",
        workspace_folder,
        "问题",
        Some("builtin-local-notes-qa"),
        true,
        "16",
        Some("test-provider"),
        Some("test-model"),
    )
    .expect("create assistant run")
}

#[test]
fn assistant_runs_allow_only_one_running_request_per_workspace() {
    let _app_data = TestAppData::new("assistant-run-unique");
    let first_session =
        db::create_assistant_session(Some("First".to_string())).expect("create first session");
    let second_session =
        db::create_assistant_session(Some("Second".to_string())).expect("create second session");

    let first = insert_assistant_run("request-a", &first_session.id, "space-a");
    let duplicate = db::create_assistant_run(
        "request-b",
        &second_session.id,
        "current",
        "space-a",
        "另一个问题",
        None,
        false,
        "16",
        Some("test-provider"),
        Some("test-model"),
    );
    let other_workspace = insert_assistant_run("request-c", &second_session.id, "space-b");

    assert_eq!(first.status, "running");
    assert!(duplicate.is_err());
    assert_eq!(other_workspace.workspace_folder, "space-b");
}

#[test]
fn assistant_run_progress_persists_partial_answer_and_turn_count() {
    let _app_data = TestAppData::new("assistant-run-progress");
    let session =
        db::create_assistant_session(Some("Progress".to_string())).expect("create session");
    insert_assistant_run("request-progress", &session.id, "space-a");

    db::append_assistant_run_delta("request-progress", "hello ").expect("append first delta");
    db::append_assistant_run_delta("request-progress", "world").expect("append second delta");
    db::update_assistant_run_turn("request-progress", 3, Some("24")).expect("update turn");
    let run = db::get_assistant_run("request-progress").expect("get assistant run");

    assert_eq!(run.partial_answer, "hello world");
    assert_eq!(run.current_turn, 3);
    assert_eq!(run.max_turns, "24");
}

#[test]
fn assistant_workspace_activity_tracks_active_and_latest_completed_session() {
    let _app_data = TestAppData::new("assistant-run-activity");
    let session =
        db::create_assistant_session(Some("Activity".to_string())).expect("create session");
    insert_assistant_run("request-active", &session.id, "space-a");

    let active = db::get_assistant_workspace_activity("space-a").expect("activity while running");
    assert_eq!(
        active
            .active_run
            .as_ref()
            .map(|run| run.request_id.as_str()),
        Some("request-active")
    );
    assert_eq!(active.latest_session_id, None);

    db::mark_assistant_run_completed("request-active", "最终回答").expect("complete run");
    let completed =
        db::get_assistant_workspace_activity("space-a").expect("activity after completion");

    assert!(completed.active_run.is_none());
    assert_eq!(
        completed.latest_session_id.as_deref(),
        Some(session.id.as_str())
    );
}

#[test]
fn assistant_run_recovery_marks_interrupted_running_requests_failed() {
    let _app_data = TestAppData::new("assistant-run-recovery");
    let session =
        db::create_assistant_session(Some("Recovery".to_string())).expect("create session");
    insert_assistant_run("request-recover", &session.id, "space-a");

    db::recover_interrupted_assistant_runs().expect("recover interrupted runs");
    let activity = db::get_assistant_workspace_activity("space-a").expect("activity");
    let run = db::get_assistant_run("request-recover").expect("get recovered run");

    assert!(activity.active_run.is_none());
    assert_eq!(run.status, "failed");
    assert!(run
        .error_message
        .unwrap_or_default()
        .contains("interrupted"));
    assert!(run.finished_at.is_some());
}

#[test]
fn assistant_schema_and_crud_validate_scope_role_and_sources() {
    let _app_data = TestAppData::new("assistant-crud");
    let session = db::create_assistant_session(Some("Test Chat".to_string()))
        .expect("create assistant session");
    let sources = vec![crate::types::AssistantSource {
        source_type: None,
        id: "space-a/note__Demo.md".to_string(),
        note_id: "note".to_string(),
        title: "Demo".to_string(),
        workspace_folder: Some("space-a".to_string()),
        url: None,
        snippet: None,
        start_line: Some(3),
        end_line: Some(5),
    }];

    let user = db::insert_assistant_message(
        &session.id,
        "user",
        "问题",
        "current",
        Some("space-a"),
        Some("test-provider"),
        Some("test-model"),
        &[],
    )
    .expect("insert user message");
    let assistant = db::insert_assistant_message(
        &session.id,
        "assistant",
        "回答",
        "current",
        Some("space-a"),
        Some("test-provider"),
        Some("test-model"),
        &sources,
    )
    .expect("insert assistant message");
    let messages = db::list_assistant_messages(&session.id).expect("list assistant messages");

    assert_eq!(user.role, "user");
    assert_eq!(assistant.sources.len(), 1);
    assert_eq!(messages.len(), 2);
    assert!(db::insert_assistant_message(
        &session.id,
        "tool",
        "bad",
        "current",
        Some("space-a"),
        None,
        None,
        &[],
    )
    .is_err());
    assert!(db::insert_assistant_message(
        &session.id,
        "assistant",
        "bad",
        "current",
        None,
        None,
        None,
        &[],
    )
    .is_err());
}

#[test]
fn assistant_sources_accept_web_sources_and_preserve_old_note_shape() {
    let _app_data = TestAppData::new("assistant-web-sources");
    let session = db::create_assistant_session(Some("Web Chat".to_string()))
        .expect("create assistant session");
    let web_sources = vec![crate::types::AssistantSource {
        source_type: Some("web".to_string()),
        id: "https://example.com/page".to_string(),
        note_id: "".to_string(),
        title: "Example Page".to_string(),
        workspace_folder: None,
        url: Some("https://example.com/page".to_string()),
        snippet: Some("网页摘要".to_string()),
        start_line: None,
        end_line: None,
    }];

    let assistant = db::insert_assistant_message(
        &session.id,
        "assistant",
        "网页回答",
        "global",
        None,
        Some("test-provider"),
        Some("test-model"),
        &web_sources,
    )
    .expect("insert assistant web source");

    assert_eq!(assistant.sources.len(), 1);
    assert_eq!(assistant.sources[0].source_type.as_deref(), Some("web"));
    assert_eq!(
        assistant.sources[0].url.as_deref(),
        Some("https://example.com/page")
    );

    let legacy_json = r#"[{"id":"space-a/notes/n1__Demo.md","note_id":"n1","title":"Demo","workspace_folder":"space-a"}]"#;
    let legacy_sources: Vec<crate::types::AssistantSource> =
        serde_json::from_str(legacy_json).expect("legacy note source JSON");
    assert_eq!(legacy_sources[0].source_type.as_deref(), Some("note"));
    assert_eq!(legacy_sources[0].note_id, "n1");
}

#[test]
fn assistant_prompt_templates_crud_rejects_builtin_edits() {
    let _app_data = TestAppData::new("assistant-prompts");
    let builtin = db::get_assistant_prompt_template("builtin-local-notes-qa")
        .expect("builtin assistant prompt");

    assert!(builtin.is_builtin);
    assert!(builtin.prompt.contains("引用来源"));
    assert!(!builtin.prompt.contains("结构化字段"));
    assert!(!builtin.prompt.contains("citations"));
    assert!(!builtin.prompt.contains("missing_evidence"));
    assert!(db::save_assistant_prompt_template(
        Some(builtin.id.clone()),
        "Edited".to_string(),
        None,
        "Prompt".to_string(),
    )
    .is_err());

    let custom = db::save_assistant_prompt_template(
        None,
        "Custom QA".to_string(),
        Some("custom".to_string()),
        "只根据笔记回答".to_string(),
    )
    .expect("save custom assistant prompt");
    let list = db::list_assistant_prompt_templates().expect("list assistant prompts");

    assert!(list.iter().any(|template| template.id == custom.id));
    db::delete_assistant_prompt_template(&custom.id).expect("delete custom assistant prompt");
    assert!(db::get_assistant_prompt_template(&custom.id).is_err());
}

#[test]
fn assistant_builtin_prompt_template_updates_when_app_initializes() {
    let _app_data = TestAppData::new("assistant-prompts-refresh");
    let conn = db::connect().expect("connect");
    conn.execute(
        "UPDATE assistant_prompt_templates SET prompt = ?1 WHERE id = 'builtin-local-notes-qa'",
        params!["最终输出必须包含结构化字段：answer、citations、missing_evidence。"],
    )
    .expect("write stale builtin prompt");

    db::init_db().expect("reinitialize DB");

    let builtin = db::get_assistant_prompt_template("builtin-local-notes-qa")
        .expect("builtin assistant prompt");
    assert!(builtin.prompt.contains("引用来源"));
    assert!(!builtin.prompt.contains("结构化字段"));
    assert!(!builtin.prompt.contains("citations"));
}

#[test]
fn assistant_messages_require_existing_session() {
    let _app_data = TestAppData::new("assistant-session-fk");

    let result = db::insert_assistant_message(
        "missing-session",
        "user",
        "问题",
        "global",
        None,
        None,
        None,
        &[],
    );

    assert!(result.is_err());
}

fn insert_assistant_history_message(
    session_id: &str,
    role: &str,
    content: &str,
) -> crate::types::AssistantMessage {
    db::insert_assistant_message(
        session_id,
        role,
        content,
        "global",
        None,
        Some("test-provider"),
        Some("test-model"),
        &[],
    )
    .expect("insert assistant history message")
}

#[test]
fn assistant_history_delete_session_removes_messages() {
    let _app_data = TestAppData::new("assistant-history-delete-session");
    let session = db::create_assistant_session(Some("History Chat".to_string()))
        .expect("create assistant session");
    insert_assistant_history_message(&session.id, "user", "first question");
    insert_assistant_history_message(&session.id, "assistant", "first answer");

    db::delete_assistant_session(&session.id).expect("delete assistant session");

    assert!(db::get_assistant_session(&session.id).is_err());
    assert!(db::list_assistant_messages(&session.id)
        .expect("list messages for deleted session")
        .is_empty());
}

#[test]
fn assistant_history_delete_session_removes_sdk_session_items() {
    let _app_data = TestAppData::new("assistant-sdk-delete-session");
    let session = db::create_assistant_session(Some("History Chat".to_string()))
        .expect("create assistant session");
    let conn = db::connect().expect("connect");
    conn.execute(
        "INSERT INTO assistant_agent_sessions (session_id) VALUES (?1)",
        params![session.id.as_str()],
    )
    .expect("insert sdk session");
    conn.execute(
        "INSERT INTO assistant_agent_items (session_id, message_data) VALUES (?1, ?2)",
        params![
            session.id.as_str(),
            "{\"role\":\"user\",\"content\":\"old\"}"
        ],
    )
    .expect("insert sdk item");

    db::delete_assistant_session(&session.id).expect("delete assistant session");

    let sdk_session_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_agent_sessions WHERE session_id = ?1",
            params![session.id.as_str()],
            |row| row.get(0),
        )
        .expect("sdk session count");
    let sdk_item_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_agent_items WHERE session_id = ?1",
            params![session.id.as_str()],
            |row| row.get(0),
        )
        .expect("sdk item count");
    assert_eq!(sdk_session_count, 0);
    assert_eq!(sdk_item_count, 0);
}

#[test]
fn assistant_history_edit_user_message_truncates_following_messages() {
    let _app_data = TestAppData::new("assistant-history-edit-user");
    let session = db::create_assistant_session(Some("History Chat".to_string()))
        .expect("create assistant session");
    let first_user = insert_assistant_history_message(&session.id, "user", "first question");
    insert_assistant_history_message(&session.id, "assistant", "first answer");
    let second_user = insert_assistant_history_message(&session.id, "user", "old question");
    insert_assistant_history_message(&session.id, "assistant", "old answer");

    let updated = db::update_assistant_user_message_and_truncate(&second_user.id, "new question")
        .expect("update user message");
    let messages = db::list_assistant_messages(&session.id).expect("list truncated messages");
    let history_before = db::recent_assistant_final_messages_before(&session.id, &updated.id, 20)
        .expect("history before edited message");

    assert_eq!(updated.id, second_user.id);
    assert_eq!(updated.content, "new question");
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].id, first_user.id);
    assert_eq!(messages[2].id, second_user.id);
    assert_eq!(messages[2].content, "new question");
    assert_eq!(history_before.len(), 2);
    assert_eq!(history_before[0].content, "first question");
    assert_eq!(history_before[1].content, "first answer");
}

#[test]
fn assistant_history_rejects_editing_assistant_message() {
    let _app_data = TestAppData::new("assistant-history-edit-assistant");
    let session = db::create_assistant_session(Some("History Chat".to_string()))
        .expect("create assistant session");
    let assistant = insert_assistant_history_message(&session.id, "assistant", "answer");

    let result = db::update_assistant_user_message_and_truncate(&assistant.id, "edited");

    assert!(result.is_err());
}

#[test]
fn assistant_history_delete_after_can_include_target_message() {
    let _app_data = TestAppData::new("assistant-history-delete-after");
    let session = db::create_assistant_session(Some("History Chat".to_string()))
        .expect("create assistant session");
    let first_user = insert_assistant_history_message(&session.id, "user", "first question");
    let second_user = insert_assistant_history_message(&session.id, "user", "delete me");
    insert_assistant_history_message(&session.id, "assistant", "delete later");

    db::delete_assistant_messages_after(&second_user.id, true)
        .expect("delete target and following messages");
    let messages = db::list_assistant_messages(&session.id).expect("list remaining messages");

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, first_user.id);
}

#[test]
fn queue_jobs_are_listed_by_workspace_and_type() {
    let app_data = TestAppData::new("queue-list");
    let recording_path = write_test_recording(&app_data, "meeting.wav");
    let recording =
        db::register_recording(&recording_path, Some("Weekly Meeting".to_string())).unwrap();
    let transcription = db::enqueue_transcription_job(
        &recording.id,
        Some("space-a"),
        None,
        serde_json::json!({"recording_title": "Weekly Meeting"}),
    )
    .expect("enqueue transcription");
    let transcript = db::insert_transcript(
        &recording.id,
        &transcription.id,
        "hello world",
        "{}",
        None,
        None,
        None,
        None,
    )
    .expect("insert transcript");
    let template = test_template();
    let summary = db::enqueue_summary_job(
        Some(&transcript.id),
        &template.id,
        "test-provider",
        "test-model",
        Some("space-a"),
        "single_transcript",
        serde_json::json!({"template_name": template.name}),
    )
    .expect("enqueue summary");

    let all = db::list_local_queue_jobs(Some("space-a"), None, None).expect("list all queue jobs");
    let transcription_only =
        db::list_local_queue_jobs(Some("space-a"), Some("transcription"), None)
            .expect("list transcription queue jobs");
    let missing_space =
        db::list_local_queue_jobs(Some("space-b"), None, None).expect("list missing space");

    assert_eq!(all.len(), 2);
    assert_eq!(transcription_only.len(), 1);
    assert_eq!(transcription_only[0].id, transcription.id);
    assert!(all
        .iter()
        .any(|job| job.id == summary.id && job.queue_type == "summary"));
    assert!(missing_space.is_empty());
}

#[test]
fn queued_jobs_initialize_pipeline_steps() {
    let app_data = TestAppData::new("pipeline-init");
    let recording_path = write_test_recording(&app_data, "pipeline.wav");
    let recording =
        db::register_recording(&recording_path, Some("Pipeline Meeting".to_string())).unwrap();
    let transcription = db::enqueue_transcription_job(
        &recording.id,
        Some("space-a"),
        None,
        serde_json::json!({"recording_title": "Pipeline Meeting"}),
    )
    .expect("enqueue transcription");
    let transcript = db::insert_transcript(
        &recording.id,
        &transcription.id,
        "hello world",
        "{}",
        None,
        None,
        None,
        None,
    )
    .expect("insert transcript");
    let template = test_template();
    let summary = db::enqueue_summary_job(
        Some(&transcript.id),
        &template.id,
        "test-provider",
        "test-model",
        Some("space-a"),
        "single_transcript",
        serde_json::json!({"template_name": template.name}),
    )
    .expect("enqueue summary");

    let transcription_steps = pipeline_steps(&transcription);
    let summary_steps = pipeline_steps(&summary);

    assert_eq!(transcription_steps.len(), 4);
    assert_eq!(
        transcription_steps[0]
            .get("name")
            .and_then(|value| value.as_str()),
        Some("prepare_audio_environment")
    );
    assert!(transcription_steps
        .iter()
        .all(|step| step.get("status").and_then(|value| value.as_str()) == Some("pending")));
    assert_eq!(
        pipeline(&transcription)
            .get("total_progress")
            .and_then(|value| value.as_i64()),
        Some(0)
    );
    assert_eq!(summary_steps.len(), 5);
    assert_eq!(
        summary_steps[0]
            .get("name")
            .and_then(|value| value.as_str()),
        Some("load_material")
    );
}

#[test]
fn transcription_pipeline_updates_on_running_success_and_failure() {
    let app_data = TestAppData::new("transcription-pipeline");
    let recording_path = write_test_recording(&app_data, "transcription.wav");
    let recording =
        db::register_recording(&recording_path, Some("Transcription Pipeline".to_string()))
            .unwrap();
    let job = db::enqueue_transcription_job(
        &recording.id,
        Some("space-a"),
        None,
        serde_json::json!({"recording_title": "Transcription Pipeline"}),
    )
    .expect("enqueue transcription");

    db::mark_transcription_job_running(&job.id, 5).expect("mark running");
    db::update_transcription_job_progress(&job.id, 35).expect("update asr progress");
    let running =
        db::get_local_queue_job("transcription", &job.id).expect("fetch running transcription");
    assert_eq!(running.status, "running");
    assert_eq!(
        step_status(&running, "prepare_audio_environment"),
        "completed"
    );
    assert_eq!(step_status(&running, "run_asr"), "running");
    assert_eq!(
        pipeline(&running)
            .get("current_step_index")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        pipeline(&running)
            .get("total_progress")
            .and_then(|value| value.as_i64()),
        Some(25)
    );

    db::finish_transcription_job(&job.id).expect("finish transcription");
    let completed =
        db::get_local_queue_job("transcription", &job.id).expect("fetch completed transcription");
    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.progress, 100);
    assert!(pipeline_steps(&completed)
        .iter()
        .all(|step| step.get("status").and_then(|value| value.as_str()) == Some("completed")));
    assert_eq!(
        pipeline(&completed)
            .get("total_progress")
            .and_then(|value| value.as_i64()),
        Some(100)
    );

    let failed_job = db::enqueue_transcription_job(
        &recording.id,
        Some("space-a"),
        None,
        serde_json::json!({"recording_title": "Transcription Pipeline"}),
    )
    .expect("enqueue failed transcription");
    db::mark_transcription_job_running(&failed_job.id, 5).expect("mark failed running");
    db::update_transcription_job_progress(&failed_job.id, 35).expect("set failed asr step");
    db::fail_transcription_job(&failed_job.id, "ASR_FAILED", "model crashed")
        .expect("fail transcription");
    let failed = db::get_local_queue_job("transcription", &failed_job.id)
        .expect("fetch failed transcription");

    assert_eq!(failed.status, "failed");
    assert_eq!(step_status(&failed, "run_asr"), "failed");
    assert_eq!(
        pipeline_step(&failed, "run_asr")
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_str()),
        Some("model crashed")
    );
}

#[test]
fn summary_pipeline_preserves_synced_metadata_and_tracks_failure() {
    let _app_data = TestAppData::new("summary-pipeline");
    let template = test_template();
    let job = db::enqueue_summary_job(
        None,
        &template.id,
        "test-provider",
        "test-model",
        Some("space-notes"),
        "workspace_text",
        serde_json::json!({
            "title": "Notes Summary",
            "documents": [{"title": "Note A", "content": "Important text"}]
        }),
    )
    .expect("enqueue summary");

    let synced = db::mark_local_queue_job_synced("summary", &job.id).expect("mark synced");
    let synced_at = synced
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("frontend_synced_at"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .expect("frontend synced marker");

    db::mark_summary_job_running(&job.id, 10).expect("mark summary running");
    db::complete_local_queue_pipeline_step("summary", &job.id, "load_material")
        .expect("complete load material");
    db::start_local_queue_pipeline_step("summary", &job.id, "call_llm", 40)
        .expect("start LLM step");
    let running = db::get_local_queue_job("summary", &job.id).expect("fetch summary running");
    assert_eq!(step_status(&running, "load_material"), "completed");
    assert_eq!(step_status(&running, "call_llm"), "running");
    assert_eq!(
        running
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("frontend_synced_at"))
            .and_then(|value| value.as_str()),
        Some(synced_at.as_str())
    );

    db::fail_summary_job(&job.id, "LLM_FAILED", "quota exceeded").expect("fail summary");
    let failed = db::get_local_queue_job("summary", &job.id).expect("fetch failed summary");
    assert_eq!(failed.status, "failed");
    assert_eq!(step_status(&failed, "call_llm"), "failed");
    assert_eq!(
        pipeline_step(&failed, "call_llm")
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_str()),
        Some("quota exceeded")
    );
    assert_eq!(
        failed
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("frontend_synced_at"))
            .and_then(|value| value.as_str()),
        Some(synced_at.as_str())
    );
}

#[test]
fn text_summary_jobs_do_not_require_transcripts() {
    let _app_data = TestAppData::new("text-summary");
    let template = test_template();
    let job = db::enqueue_summary_job(
        None,
        &template.id,
        "test-provider",
        "test-model",
        Some("space-notes"),
        "workspace_text",
        serde_json::json!({
            "title": "Notes Summary",
            "workspace_title": "Notes",
            "documents": [{"title": "Note A", "content": "Important text"}]
        }),
    )
    .expect("enqueue text summary");
    let fetched = db::get_local_queue_job("summary", &job.id).expect("fetch text summary job");

    assert_eq!(fetched.summary_scope.as_deref(), Some("workspace_text"));
    assert_eq!(fetched.transcript_id, None);
    assert_eq!(fetched.title, "Notes Summary");
}

#[test]
fn completed_jobs_can_be_marked_as_frontend_synced() {
    let _app_data = TestAppData::new("synced-marker");
    let template = test_template();
    let job = db::enqueue_summary_job(
        None,
        &template.id,
        "test-provider",
        "test-model",
        Some("space-notes"),
        "workspace_text",
        serde_json::json!({
            "title": "Notes Summary",
            "documents": [{"title": "Note A", "content": "Important text"}]
        }),
    )
    .expect("enqueue text summary");

    let marked = db::mark_local_queue_job_synced("summary", &job.id).expect("mark job synced");

    assert!(marked
        .metadata
        .as_ref()
        .and_then(|value| value.get("frontend_synced_at"))
        .and_then(|value| value.as_str())
        .is_some());
}

#[test]
fn recover_interrupted_jobs_requeues_or_completes_by_output() {
    let app_data = TestAppData::new("queue-recovery");
    let recording_path = write_test_recording(&app_data, "recovery.wav");
    let recording =
        db::register_recording(&recording_path, Some("Recovery Meeting".to_string())).unwrap();
    let pending_recovery =
        db::enqueue_transcription_job(&recording.id, Some("space-a"), None, serde_json::json!({}))
            .expect("enqueue recovery transcription");
    let finished_recovery =
        db::enqueue_transcription_job(&recording.id, Some("space-a"), None, serde_json::json!({}))
            .expect("enqueue finished transcription");
    let transcript = db::insert_transcript(
        &recording.id,
        &finished_recovery.id,
        "done",
        "{}",
        None,
        None,
        None,
        None,
    )
    .expect("insert transcript");
    db::attach_transcription_output(&finished_recovery.id, &transcript.id).expect("attach output");

    let conn = db::connect().expect("connect test DB");
    conn.execute(
        "UPDATE transcription_jobs SET status = 'running', progress = 50 WHERE id IN (?1, ?2)",
        params![pending_recovery.id, finished_recovery.id],
    )
    .expect("mark transcriptions running");
    drop(conn);

    db::recover_interrupted_queue_jobs().expect("recover interrupted jobs");

    let requeued = db::get_local_queue_job("transcription", &pending_recovery.id)
        .expect("fetch requeued transcription");
    let completed = db::get_local_queue_job("transcription", &finished_recovery.id)
        .expect("fetch completed transcription");

    assert_eq!(requeued.status, "pending");
    assert_eq!(requeued.progress, 0);
    assert!(pipeline_steps(&requeued)
        .iter()
        .all(|step| step.get("status").and_then(|value| value.as_str()) == Some("pending")));
    assert_eq!(
        pipeline(&requeued)
            .get("total_progress")
            .and_then(|value| value.as_i64()),
        Some(0)
    );
    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.progress, 100);
    assert!(pipeline_steps(&completed)
        .iter()
        .all(|step| step.get("status").and_then(|value| value.as_str()) == Some("completed")));
}
