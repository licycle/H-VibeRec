use tauri::{AppHandle, Manager};

use crate::types::{
    VoiceInputDictationResult, VoiceInputPermissionStatus, VoiceInputStats, VoiceInputStatus,
};

#[tauri::command]
pub async fn get_voice_input_stats() -> Result<VoiceInputStats, String> {
    crate::db::get_voice_input_stats()
}

#[tauri::command]
pub async fn get_voice_input_status() -> Result<VoiceInputStatus, String> {
    Ok(crate::voice_input::status())
}

#[tauri::command]
pub async fn check_voice_input_permissions() -> Result<VoiceInputPermissionStatus, String> {
    Ok(crate::voice_input::permission_status())
}

#[tauri::command]
pub async fn request_voice_input_accessibility_permission(
) -> Result<VoiceInputPermissionStatus, String> {
    Ok(crate::voice_input::request_accessibility_permission())
}

#[tauri::command]
pub async fn start_voice_input_dictation(app: AppHandle) -> Result<VoiceInputStatus, String> {
    crate::voice_input::start_dictation(app).await
}

#[tauri::command]
pub async fn stop_voice_input_dictation(
    app: AppHandle,
) -> Result<VoiceInputDictationResult, String> {
    crate::voice_input::stop_dictation(app).await
}

#[tauri::command]
pub async fn cancel_voice_input_dictation(app: AppHandle) -> Result<VoiceInputStatus, String> {
    crate::voice_input::cancel_dictation(app).await
}

#[tauri::command]
pub async fn toggle_voice_input_dictation(app: AppHandle) -> Result<VoiceInputStatus, String> {
    crate::voice_input::toggle_dictation(app).await
}

#[tauri::command]
pub async fn open_main_window_from_voice_input_overlay(app: AppHandle) -> Result<(), String> {
    log::info!("Voice input overlay requested main window open");
    let Some(main_window) = app.get_webview_window("main") else {
        return Err("Main window is unavailable".to_string());
    };
    main_window
        .show()
        .map_err(|error| format!("Failed to show main window: {error}"))?;
    main_window
        .unminimize()
        .map_err(|error| format!("Failed to unminimize main window: {error}"))?;
    main_window
        .set_focus()
        .map_err(|error| format!("Failed to focus main window: {error}"))?;
    log::info!("Voice input overlay opened main window");
    Ok(())
}

#[tauri::command]
pub async fn log_voice_input_frontend_event(event: String) -> Result<(), String> {
    let sanitized = event.replace('\n', "\\n").replace('\r', "\\r");
    log::info!("[VoiceInputFrontend] {sanitized}");
    Ok(())
}
