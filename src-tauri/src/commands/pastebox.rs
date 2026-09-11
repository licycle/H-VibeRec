use crate::{
    db, pastebox,
    types::{PasteItem, PasteMode, PasteboxState},
};
use tauri::AppHandle;

#[tauri::command]
pub async fn get_pastebox_state(
    app: AppHandle,
    before_seq: Option<i64>,
    query: Option<String>,
    pending_only: Option<bool>,
) -> Result<PasteboxState, String> {
    pastebox::get_state(
        &app,
        before_seq,
        &query.unwrap_or_default(),
        pending_only.unwrap_or(false),
    )
    .await
}
#[tauri::command]
pub async fn open_pastebox(app: AppHandle) -> Result<(), String> {
    // This button is inside our settings UI; it is not an external cursor capture action.
    pastebox::open(&app, false, None, None).await
}
#[tauri::command]
pub async fn set_pastebox_notifications_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    pastebox::set_notifications_enabled(&app, enabled).await
}
#[tauri::command]
pub async fn close_pastebox(app: AppHandle) -> Result<(), String> {
    pastebox::close(&app)
}
#[tauri::command]
pub async fn set_pastebox_mode(app: AppHandle, mode: PasteMode) -> Result<(), String> {
    pastebox::set_mode(&app, mode).await
}
#[tauri::command]
pub async fn act_on_pastebox_item(
    app: AppHandle,
    id: String,
    version: i64,
    target_id: Option<String>,
    raw: bool,
    copy_only: bool,
) -> Result<PasteItem, String> {
    pastebox::deliver(&app, &id, version, target_id, raw, false, copy_only).await
}
#[tauri::command]
pub async fn paste_next_item(app: AppHandle) -> Result<Option<PasteItem>, String> {
    let Some(item) = db::next_pending_paste_item()? else {
        return Ok(None);
    };
    pastebox::deliver_pending(&app, &item.id).await.map(Some)
}
#[tauri::command]
pub async fn delete_pastebox_item(app: AppHandle, id: String, version: i64) -> Result<(), String> {
    pastebox::delete(&app, &id, version).await
}
#[tauri::command]
pub async fn clear_pastebox_history(app: AppHandle) -> Result<u64, String> {
    pastebox::clear_history(&app).await
}
#[tauri::command]
pub async fn request_pastebox_notifications(app: AppHandle) -> Result<(), String> {
    pastebox::request_notifications(&app).await
}
#[tauri::command]
pub async fn request_pastebox_accessibility(app: AppHandle) -> Result<(), String> {
    pastebox::request_accessibility(&app).await
}
#[tauri::command]
pub async fn retry_dictation_job(
    app: AppHandle,
    id: String,
    version: i64,
) -> Result<crate::types::VoiceInputSubmission, String> {
    crate::voice_input::jobs::retry(app, &id, version).await
}

#[tauri::command]
pub async fn quit_from_pastebox(app: AppHandle) -> Result<(), String> {
    if crate::voice_input::status().phase != "idle"
        || crate::recording::is_recording()
        || db::active_dictation_count()? > 0
    {
        return Err("录音或听写仍在处理中，请完成后退出".into());
    }
    app.exit(0);
    Ok(())
}
