use std::path::PathBuf;

use uuid::Uuid;

use crate::db;
use crate::tests::support;
use crate::types::SaveSettingsRequest;

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

fn save_request_with_voice_input(
    enabled: bool,
    hotkey: &str,
    refinement_mode: &str,
) -> SaveSettingsRequest {
    let settings = db::get_settings().expect("load default settings");
    SaveSettingsRequest {
        python_path: settings.python_path,
        ffmpeg_path: settings.ffmpeg_path,
        asr_model_repo: settings.asr_model_repo,
        asr_model_source: settings.asr_model_source,
        asr_model_path: settings.asr_model_path,
        http_proxy: settings.http_proxy,
        https_proxy: settings.https_proxy,
        all_proxy: settings.all_proxy,
        use_gpu: settings.use_gpu,
        llm_provider: settings.llm_provider,
        llm_base_url: settings.llm_base_url,
        llm_model: settings.llm_model,
        llm_temperature: settings.llm_temperature,
        llm_max_tokens: settings.llm_max_tokens,
        llm_timeout_seconds: settings.llm_timeout_seconds,
        voice_input_enabled: enabled,
        voice_input_hotkey: hotkey.to_string(),
        voice_input_refinement_mode: refinement_mode.to_string(),
        voice_input_refinement_prompt: "请修正：{{ transcript }}".to_string(),
    }
}

#[test]
fn voice_input_settings_have_local_defaults_and_round_trip() {
    let _app_data = TestAppData::new("voice-input-settings");

    let defaults = db::get_settings().expect("load default settings");
    assert!(!defaults.voice_input_enabled);
    assert_eq!(defaults.voice_input_hotkey, "CommandOrControl+Shift+Space");
    assert_eq!(defaults.voice_input_refinement_mode, "local");
    assert!(defaults
        .voice_input_refinement_prompt
        .contains("{{ transcript }}"));
    assert!(defaults
        .voice_input_refinement_prompt
        .contains("补全标点符号"));
    assert!(defaults
        .voice_input_refinement_prompt
        .contains("陈述句末尾不加句号"));
    assert!(defaults
        .voice_input_refinement_prompt
        .contains("修复明显错句和语序错误"));

    let saved = db::save_settings(save_request_with_voice_input(
        true,
        "CommandOrControl+Option+V",
        "ai_polish",
    ))
    .expect("save voice input settings");

    assert!(saved.voice_input_enabled);
    assert_eq!(saved.voice_input_hotkey, "CommandOrControl+Option+V");
    assert_eq!(saved.voice_input_refinement_mode, "ai_polish");
    assert_eq!(
        saved.voice_input_refinement_prompt,
        "请修正：{{ transcript }}"
    );
}

#[test]
fn voice_input_stats_aggregate_daily_and_total_without_history() {
    let _app_data = TestAppData::new("voice-input-stats");

    db::record_voice_input_success_at("hello", "2026-05-05T09:00:00+08:00")
        .expect("record first input");
    db::record_voice_input_success_at("你好，世界", "2026-05-05T10:00:00+08:00")
        .expect("record second input");
    db::record_voice_input_success_at("昨天", "2026-05-04T10:00:00+08:00")
        .expect("record previous day input");

    let stats = db::get_voice_input_stats_for_day("2026-05-05").expect("load voice input stats");

    assert_eq!(stats.today_success_count, 2);
    assert_eq!(stats.today_success_chars, 10);
    assert_eq!(stats.total_success_count, 3);
    assert_eq!(stats.total_success_chars, 12);
    assert_eq!(
        stats.last_success_at.as_deref(),
        Some("2026-05-05T10:00:00+08:00")
    );
    assert_eq!(stats.last_success_chars, 5);
}

#[test]
fn voice_input_char_count_uses_unicode_scalar_count() {
    assert_eq!(
        crate::voice_input::text::count_inserted_chars("  hello  "),
        5
    );
    assert_eq!(
        crate::voice_input::text::count_inserted_chars("你好，世界"),
        5
    );
    assert_eq!(crate::voice_input::text::count_inserted_chars("a\nb"), 3);
}

#[test]
fn voice_input_plain_transcript_strips_workflow_labels() {
    let raw =
        "[150ms - 3515ms] Speaker 0: 现的基本功能测试一下。\n[3515ms - 4200ms] Speaker 1: 好的。";

    assert_eq!(
        crate::voice_input::text::plain_transcript_for_voice_input(raw),
        "现的基本功能测试一下。\n好的。"
    );
}

#[test]
fn voice_input_plain_transcript_compacts_cjk_character_spacing() {
    let raw = "今 天 天 气 很 好 ， 我 们 测 试 一 下 。\nOpenAI API v2 可 以 保 留 英 文 空 格 。";

    assert_eq!(
        crate::voice_input::text::plain_transcript_for_voice_input(raw),
        "今天天气很好，我们测试一下。\nOpenAI API v2 可以保留英文空格。"
    );
}

#[test]
fn voice_input_dictation_request_uses_fast_profile_without_speaker_or_vad() {
    let request = crate::voice_input::build_dictation_transcribe_request(
        "voice-input-test",
        "/tmp/input.wav",
        "/tmp/input.normalized.wav",
        "/tmp/model",
        "/tmp/ffmpeg",
        true,
        "/tmp/punc",
    );

    let payload = &request["payload"];
    assert_eq!(payload["profile"], "dictation");
    assert_eq!(payload["audio_already_normalized"], true);
    assert_eq!(payload["punc_model_path"], "/tmp/punc");
    assert!(payload.get("vad_model_path").is_none());
    assert!(payload.get("speaker_model_path").is_none());
}

#[test]
fn voice_input_dictation_warmup_request_uses_fast_profile_with_small_audio() {
    let request = crate::voice_input::build_dictation_warmup_request(
        "voice-input-warmup",
        "/tmp/model",
        true,
        "/tmp/punc",
        "/tmp/warmup.wav",
    );

    let payload = &request["payload"];
    assert_eq!(request["type"], "warmup");
    assert_eq!(payload["profile"], "dictation");
    assert_eq!(payload["model_path"], "/tmp/model");
    assert_eq!(payload["use_gpu"], true);
    assert_eq!(payload["punc_model_path"], "/tmp/punc");
    assert_eq!(payload["audio_path"], "/tmp/warmup.wav");
    assert_eq!(payload["audio_already_normalized"], true);
    assert!(payload.get("vad_model_path").is_none());
    assert!(payload.get("speaker_model_path").is_none());
}

#[test]
fn voice_input_warmup_status_event_carries_reason_and_timing() {
    let event = crate::voice_input::build_warmup_status_event(
        "ready",
        "本地转写模型已就绪",
        "startup",
        Some(9101),
        Some(1189),
    );

    assert_eq!(event.phase, "ready");
    assert_eq!(event.message, "本地转写模型已就绪");
    assert_eq!(event.reason, "startup");
    assert_eq!(event.elapsed_ms, Some(9101));
    assert_eq!(event.sidecar_infer_ms, Some(1189));
}

#[test]
fn voice_input_startup_warmup_requires_enabled_and_no_meeting_recording() {
    let _app_data = TestAppData::new("voice-input-startup-warmup");
    let mut settings = db::get_settings().expect("load settings");

    settings.voice_input_enabled = true;
    assert!(crate::voice_input::should_startup_dictation_warmup(
        &settings, false
    ));
    assert!(!crate::voice_input::should_startup_dictation_warmup(
        &settings, true
    ));

    settings.voice_input_enabled = false;
    assert!(!crate::voice_input::should_startup_dictation_warmup(
        &settings, false
    ));
}

#[test]
fn voice_input_settings_change_warmup_runs_for_enable_or_model_change() {
    let _app_data = TestAppData::new("voice-input-settings-warmup");
    let mut previous = db::get_settings().expect("load previous settings");
    let mut saved = previous.clone();

    previous.voice_input_enabled = false;
    saved.voice_input_enabled = true;
    assert!(
        crate::voice_input::should_schedule_dictation_warmup_after_settings_change(
            &previous, &saved
        )
    );

    previous.voice_input_enabled = true;
    saved.voice_input_enabled = true;
    saved.asr_model_repo = "paraformer-zh-alt".to_string();
    assert!(
        crate::voice_input::should_schedule_dictation_warmup_after_settings_change(
            &previous, &saved
        )
    );

    saved.asr_model_repo = previous.asr_model_repo.clone();
    saved.voice_input_enabled = false;
    assert!(
        !crate::voice_input::should_schedule_dictation_warmup_after_settings_change(
            &previous, &saved
        )
    );

    saved.voice_input_enabled = true;
    assert!(
        !crate::voice_input::should_schedule_dictation_warmup_after_settings_change(
            &previous, &saved
        )
    );
}

#[test]
fn voice_input_polish_failure_falls_back_to_raw_text() {
    let outcome = crate::voice_input::voice_input_text_after_polish(
        "原始转写",
        Err("LLM timeout".to_string()),
    );

    assert_eq!(outcome.text, "原始转写");
    assert!(outcome.fallback);
    assert_eq!(outcome.error.as_deref(), Some("LLM timeout"));
}

#[test]
fn voice_input_hotkey_parser_accepts_common_macos_shortcuts() {
    let parsed = crate::voice_input::hotkey::parse_hotkey("CommandOrControl+Shift+Space")
        .expect("parse default hotkey");

    assert!(parsed.command);
    assert!(parsed.shift);
    assert!(!parsed.option);
    assert!(!parsed.control);
    assert_eq!(parsed.key_code, 49);
    assert_eq!(parsed.carbon_key_code(), 49);
    assert_eq!(
        parsed.carbon_modifiers(),
        crate::voice_input::hotkey::CARBON_CMD_KEY | crate::voice_input::hotkey::CARBON_SHIFT_KEY
    );
}

#[test]
fn voice_input_hotkey_parser_maps_control_and_option_for_global_registration() {
    let parsed = crate::voice_input::hotkey::parse_hotkey("CommandOrControl+Control+Option+V")
        .expect("parse multi-modifier hotkey");

    assert!(parsed.command);
    assert!(!parsed.shift);
    assert!(parsed.option);
    assert!(parsed.control);
    assert_eq!(parsed.key_code, 9);
    assert_eq!(
        parsed.carbon_modifiers(),
        crate::voice_input::hotkey::CARBON_CMD_KEY
            | crate::voice_input::hotkey::CARBON_OPTION_KEY
            | crate::voice_input::hotkey::CARBON_CONTROL_KEY
    );
}

#[test]
fn voice_input_hotkey_parser_rejects_single_plain_key() {
    let error =
        crate::voice_input::hotkey::parse_hotkey("A").expect_err("single plain key should fail");

    assert!(error.contains("modifier"));
}

#[test]
fn voice_input_hotkey_parser_accepts_modifier_only_hotkeys() {
    let parsed = crate::voice_input::hotkey::parse_hotkey("CommandOrControl+Option")
        .expect("parse modifier-only hotkey");

    assert!(parsed.command);
    assert!(!parsed.shift);
    assert!(parsed.option);
    assert!(!parsed.control);
    assert!(parsed.is_modifier_only());
    assert_eq!(
        parsed.carbon_modifiers(),
        crate::voice_input::hotkey::CARBON_CMD_KEY | crate::voice_input::hotkey::CARBON_OPTION_KEY
    );
}

#[test]
fn voice_input_hotkey_parser_accepts_single_modifier_hotkey() {
    let parsed =
        crate::voice_input::hotkey::parse_hotkey("Control").expect("parse single modifier hotkey");

    assert!(!parsed.command);
    assert!(!parsed.shift);
    assert!(!parsed.option);
    assert!(parsed.control);
    assert!(parsed.is_modifier_only());
    assert_eq!(
        parsed.carbon_modifiers(),
        crate::voice_input::hotkey::CARBON_CONTROL_KEY
    );
}

#[test]
fn voice_input_hotkey_parser_rejects_shift_only_hotkey() {
    let error =
        crate::voice_input::hotkey::parse_hotkey("Shift").expect_err("shift-only should fail");

    assert!(error.contains("Command, Option, or Control"));
}

#[test]
fn voice_input_hotkey_display_names_macos_shortcuts() {
    assert_eq!(
        crate::voice_input::hotkey::display_hotkey_for_status("CommandOrControl+Shift+Space"),
        "⌘⇧Space"
    );
    assert_eq!(
        crate::voice_input::hotkey::display_hotkey_for_status("CommandOrControl+Option+V"),
        "⌘⌥V"
    );
    assert_eq!(
        crate::voice_input::hotkey::display_hotkey_for_status("Control"),
        "⌃"
    );
}

#[test]
fn voice_input_enter_key_codes_cover_return_and_keypad_enter() {
    assert!(crate::voice_input::is_enter_key_code(36));
    assert!(crate::voice_input::is_enter_key_code(76));
    assert!(!crate::voice_input::is_enter_key_code(49));
}

#[test]
fn voice_input_listening_message_uses_enter_only_when_available() {
    let enter_message = crate::voice_input::listening_status_message("⌘⇧Space", true);
    assert_eq!(enter_message, "正在听写 · Enter 完成 · Esc 取消");

    let fallback_message = crate::voice_input::listening_status_message("⌘⇧Space", false);
    assert_eq!(fallback_message, "正在听写");
}

#[test]
fn voice_input_overlay_position_uses_work_area_above_dock() {
    let (x, y) = crate::voice_input::overlay_position_for_work_area(0, 25, 1440, 800, 560, 88);

    assert_eq!(x, 440);
    assert_eq!(y, 703);
}

#[test]
fn voice_input_settings_reject_invalid_hotkey_before_saving() {
    let _app_data = TestAppData::new("voice-input-invalid-hotkey");

    let error = db::save_settings(save_request_with_voice_input(
        true,
        "CommandOrControl+Nope",
        "local",
    ))
    .expect_err("invalid hotkey should not be persisted");

    assert!(error.contains("unsupported hotkey key"));
}

#[test]
fn voice_input_accessibility_dev_hint_names_the_dev_binary() {
    let hint = crate::voice_input::accessibility_permission_hint(true);

    assert!(hint.contains("target/debug/hit-vvc"));
    assert!(hint.contains("Code"));
    assert!(hint.contains("Terminal"));
}

#[test]
fn voice_input_accessibility_replaces_utf16_selection_ranges() {
    let replaced = crate::voice_input::insertion::replace_utf16_range("hello 世界", 6, 2, "Rust")
        .expect("replace Chinese range");

    assert_eq!(replaced, "hello Rust");

    let emoji_replaced = crate::voice_input::insertion::replace_utf16_range("a😀b", 1, 2, "🙂")
        .expect("replace surrogate pair");

    assert_eq!(emoji_replaced, "a🙂b");
}

#[test]
fn voice_input_skips_ax_value_insertion_for_own_app_focus() {
    assert!(
        !crate::voice_input::insertion::should_use_ax_value_insertion(
            Some(42),
            42,
            Some("hit-vvc")
        )
    );
    assert!(!crate::voice_input::insertion::should_use_ax_value_insertion(Some(7), 42, None));
}

#[test]
fn voice_input_uses_clipboard_paste_for_electron_editors() {
    assert!(crate::voice_input::insertion::should_use_clipboard_paste_for_process_name("Code"));
    assert!(crate::voice_input::insertion::should_use_clipboard_paste_for_process_name("Cursor"));
    assert!(
        crate::voice_input::insertion::should_use_clipboard_paste_for_process_name(
            "Visual Studio Code"
        )
    );
    assert!(
        !crate::voice_input::insertion::should_use_clipboard_paste_for_process_name("TextEdit")
    );
}

#[test]
fn voice_input_defaults_external_apps_to_clipboard_paste() {
    assert!(
        !crate::voice_input::insertion::should_use_ax_value_insertion(Some(7), 42, Some("Slack"))
    );
    assert!(
        !crate::voice_input::insertion::should_use_ax_value_insertion(
            Some(7),
            42,
            Some("Unknown App")
        )
    );
    assert!(crate::voice_input::insertion::should_use_clipboard_paste_for_process_name("Slack"));
    assert!(
        crate::voice_input::insertion::should_use_clipboard_paste_for_process_name("Unknown App")
    );
}

#[test]
fn voice_input_allows_ax_value_only_for_verified_native_apps() {
    assert!(
        crate::voice_input::insertion::should_use_ax_value_insertion(Some(7), 42, Some("TextEdit"))
    );
}
