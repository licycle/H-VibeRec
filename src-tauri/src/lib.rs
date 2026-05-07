// Module declarations
mod asr_worker;
mod audio;
mod commands;
mod db;
mod files;
mod llm;
mod local_queue;
mod recording;
mod sidecar;
mod storage;
#[cfg(test)]
mod tests;
mod types;
mod voice_input;

use log::{info, warn};
use tauri::{Manager, Runtime, WebviewWindow};

// Import all command functions
use commands::{
    ask_local_notes_agent,
    ask_local_notes_agent_stream,
    cancel_asr_model_download,
    cancel_local_queue_job,
    cancel_voice_input_dictation,
    check_audio_devices,
    // Local app commands
    check_runtime_dependencies,
    check_voice_input_permissions,
    delete_assistant_messages_after,
    delete_assistant_prompt_template,
    delete_assistant_session,
    // File commands
    delete_audio_file,
    delete_recording,
    delete_summary_template,
    delete_workspace_dir,
    delete_workspace_note,
    delete_workspace_recording,
    enqueue_summary,
    enqueue_transcription,
    enqueue_workspace_summary,
    enqueue_workspace_text_summary,
    ensure_asr_model,
    ensure_workspace_dir,
    export_audio_file,
    export_summary,
    export_transcript,
    get_assistant_workspace_activity,
    get_latest_transcript,
    get_model_download_progress,
    get_model_status,
    get_recording_info,
    get_settings,
    get_summary,
    get_transcript,
    get_voice_input_stats,
    get_voice_input_status,
    get_workspace_recording_save_path,
    has_llm_api_key,
    import_audio_files_to_workspace,
    import_note_files,
    is_recording,
    list_assistant_messages,
    list_assistant_prompt_templates,
    list_assistant_sessions,
    list_local_queue_jobs,
    list_recordings_with_status,
    list_summary_templates,
    list_workspace_dirs,
    list_workspace_notes,
    list_workspace_recordings,
    log_voice_input_frontend_event,
    mark_local_queue_job_synced,
    open_audio_permission_settings,
    open_external_url,
    open_main_window_from_voice_input_overlay,
    play_audio_file,
    register_recording,
    // Device commands
    request_microphone_permission,
    request_voice_input_accessibility_permission,
    retry_summary,
    retry_transcription,
    save_assistant_answer_note,
    save_assistant_prompt_template,
    save_assistant_session_note,
    save_settings,
    save_summary_template,
    save_text_file,
    save_workspace_note,
    set_llm_api_key,
    // Recording commands
    start_recording,
    start_voice_input_dictation,
    stop_recording,
    stop_voice_input_dictation,
    summarize_transcript,
    summarize_workspace_texts,
    summarize_workspace_transcripts,
    test_llm_provider,
    toggle_voice_input_dictation,
    transcribe_recording,
    update_assistant_user_message_and_truncate,
    verify_audio_permissions,
};

const MAIN_WINDOW_LABEL: &str = "main";

trait MainWindowLifecycle {
    fn show(&self) -> Result<(), String>;
    fn unminimize(&self) -> Result<(), String>;
    fn set_focus(&self) -> Result<(), String>;
}

impl<R: Runtime> MainWindowLifecycle for WebviewWindow<R> {
    fn show(&self) -> Result<(), String> {
        WebviewWindow::show(self).map_err(|error| format!("Failed to show main window: {error}"))
    }

    fn unminimize(&self) -> Result<(), String> {
        WebviewWindow::unminimize(self)
            .map_err(|error| format!("Failed to unminimize main window: {error}"))
    }

    fn set_focus(&self) -> Result<(), String> {
        WebviewWindow::set_focus(self)
            .map_err(|error| format!("Failed to focus main window: {error}"))
    }
}

fn restore_main_window(window: &impl MainWindowLifecycle) -> Result<(), String> {
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    Ok(())
}

fn restore_main_window_from_app<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(main_window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        warn!("Failed to restore main window on reopen: main window is unavailable");
        return;
    };

    if let Err(error) = restore_main_window(&main_window) {
        warn!("Failed to restore main window on reopen: {error}");
    }
}

/// Main entry point for the Tauri application
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            crate::db::init_db().map_err(|error| {
                log::error!("Failed to initialize local database: {}", error);
                error
            })?;
            crate::db::recover_interrupted_assistant_runs().map_err(|error| {
                log::error!("Failed to recover assistant runs: {}", error);
                error
            })?;
            crate::local_queue::init(app.handle().clone());
            crate::voice_input::init(app.handle().clone());
            info!("hit-vvc application setup complete");
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW_LABEL {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        warn!("Failed to hide main window on close request: {error}");
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Recording commands
            start_recording,
            stop_recording,
            is_recording,
            get_recording_info,
            start_voice_input_dictation,
            stop_voice_input_dictation,
            cancel_voice_input_dictation,
            toggle_voice_input_dictation,
            open_main_window_from_voice_input_overlay,
            get_voice_input_status,
            get_voice_input_stats,
            check_voice_input_permissions,
            request_voice_input_accessibility_permission,
            log_voice_input_frontend_event,
            // File commands
            list_workspace_recordings,
            list_workspace_dirs,
            ensure_workspace_dir,
            list_workspace_notes,
            save_workspace_note,
            delete_workspace_note,
            delete_workspace_recording,
            delete_audio_file,
            play_audio_file,
            export_audio_file,
            save_text_file,
            import_audio_files_to_workspace,
            import_note_files,
            get_workspace_recording_save_path,
            delete_workspace_dir,
            ask_local_notes_agent,
            ask_local_notes_agent_stream,
            get_assistant_workspace_activity,
            list_assistant_sessions,
            list_assistant_messages,
            list_assistant_prompt_templates,
            save_assistant_prompt_template,
            delete_assistant_prompt_template,
            delete_assistant_session,
            delete_assistant_messages_after,
            update_assistant_user_message_and_truncate,
            save_assistant_answer_note,
            save_assistant_session_note,
            open_external_url,
            // Device commands
            request_microphone_permission,
            verify_audio_permissions,
            check_audio_devices,
            open_audio_permission_settings,
            // Local app commands
            cancel_asr_model_download,
            cancel_local_queue_job,
            check_runtime_dependencies,
            delete_recording,
            delete_summary_template,
            ensure_asr_model,
            enqueue_summary,
            enqueue_transcription,
            enqueue_workspace_summary,
            enqueue_workspace_text_summary,
            export_summary,
            export_transcript,
            get_latest_transcript,
            get_model_download_progress,
            get_model_status,
            get_settings,
            get_summary,
            get_transcript,
            has_llm_api_key,
            list_local_queue_jobs,
            list_recordings_with_status,
            list_summary_templates,
            mark_local_queue_job_synced,
            register_recording,
            retry_summary,
            retry_transcription,
            save_settings,
            save_summary_template,
            set_llm_api_key,
            summarize_transcript,
            summarize_workspace_texts,
            summarize_workspace_transcripts,
            test_llm_provider,
            transcribe_recording,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                restore_main_window_from_app(app_handle);
            }
        });
}

#[cfg(test)]
mod window_lifecycle_tests {
    use std::cell::RefCell;

    struct FakeMainWindow {
        calls: RefCell<Vec<&'static str>>,
    }

    impl FakeMainWindow {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl super::MainWindowLifecycle for FakeMainWindow {
        fn show(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("show");
            Ok(())
        }

        fn unminimize(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("unminimize");
            Ok(())
        }

        fn set_focus(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("set_focus");
            Ok(())
        }
    }

    #[test]
    fn restore_main_window_shows_unminimizes_and_focuses_window() {
        let window = FakeMainWindow::new();

        super::restore_main_window(&window).expect("restore main window");

        assert_eq!(
            window.calls.borrow().as_slice(),
            ["show", "unminimize", "set_focus"]
        );
    }
}
