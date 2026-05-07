pub mod assistant;
pub mod devices;
pub mod files;
pub mod local;
pub mod recording;
pub mod voice_input;

// Re-export all command functions
pub use assistant::{
    ask_local_notes_agent, ask_local_notes_agent_stream, delete_assistant_messages_after,
    delete_assistant_prompt_template, delete_assistant_session, get_assistant_workspace_activity,
    list_assistant_messages, list_assistant_prompt_templates, list_assistant_sessions,
    open_external_url, save_assistant_answer_note, save_assistant_prompt_template,
    save_assistant_session_note, update_assistant_user_message_and_truncate,
};
pub use devices::{
    check_audio_devices, open_audio_permission_settings, request_microphone_permission,
    verify_audio_permissions,
};
pub use files::{
    delete_audio_file, delete_workspace_dir, delete_workspace_note, delete_workspace_recording,
    ensure_workspace_dir, export_audio_file, get_workspace_recording_save_path,
    import_audio_files_to_workspace, import_note_files, list_workspace_dirs, list_workspace_notes,
    list_workspace_recordings, play_audio_file, save_text_file, save_workspace_note,
};
pub use local::{
    cancel_asr_model_download, cancel_local_queue_job, check_runtime_dependencies,
    delete_recording, delete_summary_template, enqueue_summary, enqueue_transcription,
    enqueue_workspace_summary, enqueue_workspace_text_summary, ensure_asr_model, export_summary,
    export_transcript, get_latest_transcript, get_model_download_progress, get_model_status,
    get_settings, get_summary, get_transcript, has_llm_api_key, list_local_queue_jobs,
    list_recordings_with_status, list_summary_templates, mark_local_queue_job_synced,
    register_recording, retry_summary, retry_transcription, save_settings, save_summary_template,
    set_llm_api_key, summarize_transcript, summarize_workspace_texts,
    summarize_workspace_transcripts, test_llm_provider, transcribe_recording,
};
pub use recording::{get_recording_info, is_recording, start_recording, stop_recording};
pub use voice_input::{
    cancel_voice_input_dictation, check_voice_input_permissions, get_voice_input_stats,
    get_voice_input_status, log_voice_input_frontend_event,
    open_main_window_from_voice_input_overlay, request_voice_input_accessibility_permission,
    start_voice_input_dictation, stop_voice_input_dictation, toggle_voice_input_dictation,
};
