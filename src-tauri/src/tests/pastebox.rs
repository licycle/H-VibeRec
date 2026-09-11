use crate::{db, types::PasteMode};
use std::path::PathBuf;
use uuid::Uuid;

struct TestDb {
    _guard: std::sync::MutexGuard<'static, ()>,
    path: PathBuf,
}
impl TestDb {
    fn new() -> Self {
        let guard = super::support::lock_app_data();
        let path = std::env::temp_dir().join(format!("hvr-pastebox-test-{}", Uuid::new_v4()));
        std::env::set_var("VOICE_VIBE_TEST_APP_DATA_DIR", &path);
        db::init_db().unwrap();
        Self {
            _guard: guard,
            path,
        }
    }
}
impl Drop for TestDb {
    fn drop(&mut self) {
        std::env::remove_var("VOICE_VIBE_TEST_APP_DATA_DIR");
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn async_jobs_allocate_ids_and_order_before_transcription_and_preserve_modes() {
    let _db = TestDb::new();
    let a = db::insert_dictation_job(&Uuid::new_v4().to_string(), PasteMode::Automatic).unwrap();
    let b = db::insert_dictation_job(&Uuid::new_v4().to_string(), PasteMode::Notify).unwrap();
    assert!(a.seq < b.seq);
    assert_eq!(db::active_dictation_count().unwrap(), 2);
    assert!(db::claim_paste_item(&a.id, a.version, false).is_err());
    assert!(db::delete_paste_item(&a.id, a.version).is_err());
    db::set_pastebox_mode(PasteMode::Manual).unwrap();
    db::finish_dictation_transcription(&b.id, "B先完成", false).unwrap();
    db::finish_dictation_transcription(&a.id, "A原文", true).unwrap();
    db::finish_paste_refinement(&a.id, "A润色", None).unwrap();
    let a2 = db::get_paste_item(&a.id).unwrap();
    let b2 = db::get_paste_item(&b.id).unwrap();
    assert_eq!(
        (a2.id, a2.seq, a2.mode),
        (a.id, a.seq, PasteMode::Automatic)
    );
    assert_eq!((b2.id, b2.seq, b2.mode), (b.id, b.seq, PasteMode::Notify));
    assert_eq!(db::active_dictation_count().unwrap(), 0);
}

#[test]
fn later_automatic_job_waits_for_earlier_polish_but_not_failure_or_manual_jobs() {
    let _db = TestDb::new();
    let a = db::insert_dictation_job(&Uuid::new_v4().to_string(), PasteMode::Automatic).unwrap();
    let b = db::insert_dictation_job(&Uuid::new_v4().to_string(), PasteMode::Automatic).unwrap();
    db::finish_dictation_transcription(&a.id, "A", true).unwrap();
    db::finish_dictation_transcription(&b.id, "B", false).unwrap();
    assert!(db::has_earlier_automatic_job(b.seq).unwrap());
    let a = db::get_paste_item(&a.id).unwrap();
    let (_, lease) = db::claim_paste_item(&a.id, a.version, false).unwrap();
    db::finish_paste_delivery(&a.id, &lease, "copied", None).unwrap();
    assert!(!db::has_earlier_automatic_job(b.seq).unwrap());
    db::fail_dictation_job(&b.id, "test failure").unwrap();
    let _manual = db::insert_dictation_job(&Uuid::new_v4().to_string(), PasteMode::Manual).unwrap();
    let c = db::insert_dictation_job(&Uuid::new_v4().to_string(), PasteMode::Automatic).unwrap();
    assert!(!db::has_earlier_automatic_job(c.seq).unwrap());
}

#[test]
fn failed_asr_retry_is_single_use_and_keeps_its_identity() {
    let _db = TestDb::new();
    let a = db::insert_dictation_job(&Uuid::new_v4().to_string(), PasteMode::Manual).unwrap();
    db::fail_dictation_job(&a.id, "ASR timeout").unwrap();
    let failed = db::get_paste_item(&a.id).unwrap();
    assert!(db::claim_paste_item(&failed.id, failed.version, false).is_err());
    let retry = db::retry_dictation_job(&failed.id, failed.version).unwrap();
    assert_eq!(retry.seq, a.seq);
    assert_eq!(retry.id, a.id);
    assert!(retry.error.is_none());
    assert!(db::retry_dictation_job(&failed.id, failed.version).is_err());
    db::finish_dictation_transcription(&retry.id, "恢复文本", false).unwrap();
    let ready = db::get_paste_item(&retry.id).unwrap();
    assert!(db::retry_dictation_job(&ready.id, ready.version).is_err());
}

#[test]
fn restart_marks_interrupted_asr_retryable_and_never_automatically_replays_old_targets() {
    let _db = TestDb::new();
    let a = db::insert_dictation_job(&Uuid::new_v4().to_string(), PasteMode::Automatic).unwrap();
    let b = db::insert_dictation_job(&Uuid::new_v4().to_string(), PasteMode::Automatic).unwrap();
    db::set_dictation_stage(&a.id, "transcribing").unwrap();
    db::finish_dictation_transcription(&b.id, "已有文字", true).unwrap();
    db::recover_pastebox().unwrap();
    let a = db::get_paste_item(&a.id).unwrap();
    let b = db::get_paste_item(&b.id).unwrap();
    assert_eq!(a.processing_status, "failed");
    assert_eq!(b.processing_status, "ready");
    assert_eq!(b.delivery_status, "failed");
    assert_eq!(b.text, "已有文字");
    assert!(!db::has_earlier_automatic_job(b.seq + 1).unwrap());
    assert_eq!(db::active_dictation_count().unwrap(), 0);
}

#[test]
fn notification_setting_defaults_on_and_persists_without_changing_mode() {
    let _db = TestDb::new();
    assert!(db::pastebox_notifications_enabled().unwrap());
    db::set_pastebox_mode(PasteMode::Notify).unwrap();
    db::set_pastebox_notifications_enabled(false).unwrap();
    db::connect().unwrap().execute(
        "INSERT INTO app_settings (key,value,updated_at) VALUES ('pastebox_shortcuts','{}','test')", [],
    ).unwrap();
    db::init_db().unwrap();
    let old_count: i64 = db::connect()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM app_settings WHERE key = 'pastebox_shortcuts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(old_count, 0);
    assert!(!db::pastebox_notifications_enabled().unwrap());
    assert_eq!(db::pastebox_mode().unwrap(), PasteMode::Notify);
    db::set_pastebox_notifications_enabled(true).unwrap();
    assert!(db::pastebox_notifications_enabled().unwrap());
}

#[test]
fn outbox_migration_preserves_settings_and_raw_transcripts() {
    let _db = TestDb::new();
    db::set_pastebox_mode(PasteMode::Notify).unwrap();
    let raw = db::insert_paste_item("中文😀\n第二行", PasteMode::Notify, None, true).unwrap();
    db::init_db().unwrap();
    let saved = db::get_paste_item(&raw.id).unwrap();
    assert_eq!(saved.raw_text, "中文😀\n第二行");
    assert_eq!(saved.text, saved.raw_text);
    assert_eq!(db::pastebox_mode().unwrap(), PasteMode::Notify);
    let failed =
        db::finish_paste_refinement(&raw.id, &raw.raw_text, Some("LLM unavailable")).unwrap();
    assert_eq!(failed.processing_status, "ready");
    assert_eq!(failed.polish_error.as_deref(), Some("LLM unavailable"));
    assert_eq!(failed.delivery_status, "pending");
}

#[test]
fn notifications_can_be_actioned_out_of_order_by_id_only_once() {
    let _db = TestDb::new();
    let a = db::insert_paste_item("内容 A", PasteMode::Notify, None, false).unwrap();
    let b = db::insert_paste_item("内容 B", PasteMode::Notify, None, false).unwrap();
    assert!(a.seq < b.seq);
    let (claimed_b, token_b) = db::claim_paste_item(&b.id, b.version, true).unwrap();
    assert_eq!(claimed_b.text, "内容 B");
    db::finish_paste_delivery(&b.id, &token_b, "verified", None).unwrap();
    assert!(db::claim_paste_item(&b.id, db::get_paste_item(&b.id).unwrap().version, true).is_err());
    assert_eq!(db::next_pending_paste_item().unwrap().unwrap().id, a.id);
    let (claimed_a, token_a) = db::claim_paste_item(&a.id, a.version, true).unwrap();
    assert_eq!(claimed_a.text, "内容 A");
    assert_ne!(token_a, token_b);
    assert!(db::finish_paste_delivery(&a.id, &token_b, "verified", None).is_err());
    db::finish_paste_delivery(&a.id, &token_a, "verified", None).unwrap();
    assert!(db::next_pending_paste_item().unwrap().is_none());
}

#[test]
fn using_raw_text_while_refining_consumes_automatic_action_but_keeps_final_text() {
    let _db = TestDb::new();
    let item = db::insert_paste_item("原文", PasteMode::Automatic, None, true).unwrap();
    let (_, token) = db::claim_paste_item(&item.id, item.version, false).unwrap();
    let refined = db::finish_paste_refinement(&item.id, "润色文字", None).unwrap();
    assert!(refined.actioned);
    // Refinement increments the version but must not invalidate the delivery's lease.
    let copied = db::finish_paste_delivery(&item.id, &token, "copied", None).unwrap();
    assert_eq!(copied.text, "润色文字");
    assert_eq!(copied.raw_text, "原文");
    assert!(db::claim_paste_item(&item.id, copied.version, true).is_err());
    assert_eq!(db::pending_paste_count().unwrap(), 0);
    // A deliberate new user action may reuse history, with a fresh revision.
    assert!(db::claim_paste_item(&item.id, copied.version, false).is_ok());
}

#[test]
fn stale_clicks_and_concurrent_deletes_cannot_repeat_a_delivery() {
    let _db = TestDb::new();
    let item = db::insert_paste_item("一条内容", PasteMode::Manual, None, false).unwrap();
    let (_, token) = db::claim_paste_item(&item.id, item.version, false).unwrap();
    assert!(db::claim_paste_item(&item.id, item.version, false).is_err());
    assert!(
        db::delete_paste_item(&item.id, db::get_paste_item(&item.id).unwrap().version).is_err()
    );
    let failed =
        db::finish_paste_delivery(&item.id, &token, "failed", Some("target closed")).unwrap();
    assert_eq!(failed.error.as_deref(), Some("target closed"));
    assert!(db::delete_paste_item(&item.id, item.version).is_err());
    assert_eq!(db::pending_paste_count().unwrap(), 1);
    db::delete_paste_item(&item.id, failed.version).unwrap();
    assert!(db::get_paste_item(&item.id).is_err());
}

#[test]
fn restart_preserves_pending_items_and_never_replays_uncertain_delivery() {
    let _db = TestDb::new();
    let refining = db::insert_paste_item("还在润色", PasteMode::Automatic, None, true).unwrap();
    let delivering = db::insert_paste_item("已发按键", PasteMode::Notify, None, false).unwrap();
    db::claim_paste_item(&delivering.id, delivering.version, true).unwrap();
    db::recover_pastebox().unwrap();
    let recovered = db::get_paste_item(&refining.id).unwrap();
    assert_eq!(recovered.processing_status, "ready");
    assert_eq!(recovered.raw_text, "还在润色");
    assert!(recovered.polish_error.is_some());
    let uncertain = db::get_paste_item(&delivering.id).unwrap();
    assert_eq!(uncertain.delivery_status, "uncertain");
    assert!(uncertain.actioned);
    assert!(db::claim_paste_item(&uncertain.id, uncertain.version, true).is_err());
}

#[test]
fn history_paginates_without_deleting_pending_records_and_searches_raw_and_final() {
    let _db = TestDb::new();
    for i in 0..61 {
        db::insert_paste_item(&format!("记录 {i}"), PasteMode::Manual, None, false).unwrap();
    }
    let first = db::list_paste_items(None, "", true).unwrap();
    let second = db::list_paste_items(Some(first.last().unwrap().seq), "", true).unwrap();
    assert_eq!(first.len(), 50);
    assert_eq!(second.len(), 11);
    assert!(first.iter().all(|a| !second.iter().any(|b| a.id == b.id)));
    assert_eq!(db::pending_paste_count().unwrap(), 61);
    let item = &first[0];
    db::finish_paste_refinement(&item.id, "独特润色词", None).unwrap();
    assert_eq!(
        db::list_paste_items(None, "独特润色词", false).unwrap()[0].id,
        item.id
    );
    assert_eq!(
        db::list_paste_items(None, &item.seq.to_string(), false).unwrap()[0].id,
        item.id
    );
    assert_eq!(
        db::list_paste_items(None, &item.raw_text, false).unwrap()[0].id,
        item.id
    );
}
