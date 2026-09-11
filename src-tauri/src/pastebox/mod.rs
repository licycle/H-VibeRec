#[cfg(all(debug_assertions, target_os = "macos"))]
pub(crate) mod audit;
mod dictation;
mod focus;
mod native;
mod notifications;
pub use dictation::DictationContext;
#[cfg(target_os = "macos")]
mod worker;

use crate::{
    db,
    types::{PasteItem, PasteMode, PasteReceipt, PasteTarget, PasteboxState},
};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

pub const WINDOW_LABEL: &str = "pastebox";
static APP: OnceLock<AppHandle> = OnceLock::new();
static CONTEXT: Mutex<PanelContext> = Mutex::new(PanelContext {
    targets: Vec::new(),
    opening_target_id: None,
    requested_item_id: None,
    target_error: None,
});
static DELIVERY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static FOCUS: std::sync::LazyLock<std::sync::Arc<focus::FocusGate>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(focus::FocusGate::default()));
static NOTIFICATIONS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn reserve_delivery_focus(
    automatic: bool,
) -> (focus::WriteLease, tokio::sync::MutexGuard<'static, ()>) {
    loop {
        let focus = FOCUS.write(automatic).await;
        if let Ok(delivery) = DELIVERY.try_lock() {
            return (focus, delivery);
        }
        // Native notification acknowledgements may hold DELIVERY for seconds.
        // Do not deny a recording while simply waiting for that acknowledgement.
        drop(focus);
        drop(DELIVERY.lock().await);
    }
}
static NOTIFICATION_RESULTS: std::sync::LazyLock<
    Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<Result<(), String>>>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

struct PanelContext {
    targets: Vec<PasteTarget>,
    opening_target_id: Option<String>,
    requested_item_id: Option<String>,
    target_error: Option<String>,
}

pub fn init(app: AppHandle) -> Result<(), String> {
    db::recover_pastebox()?;
    let _ = APP.set(app.clone());
    native::init(&app);
    tauri::async_runtime::spawn(async move {
        let _lock = NOTIFICATIONS.lock().await;
        let enabled = db::pastebox_notifications_enabled().unwrap_or(true);
        let _ = native::call(
            &app,
            json!({"op":"notifications_enabled", "enabled":enabled}),
        )
        .await;
        // Upgrades may already be in automatic/notify mode. Request the first
        // OS authorization without making the user switch modes again.
        if enabled && db::pastebox_mode().unwrap_or_default() != PasteMode::Manual {
            let _ = request_notifications(&app).await;
        }
        changed(&app).await;
        let _ = accessibility_status(&app).await;
        let _ = app.emit("pastebox-changed", ());
    });
    Ok(())
}

fn remember(target: PasteTarget) {
    if let Ok(mut context) = CONTEXT.lock() {
        context.targets.retain(|t| t.id != target.id);
        context.targets.insert(0, target);
        context.targets.truncate(10);
        context.target_error = None;
    }
}

pub async fn capture(app: &AppHandle) -> Result<PasteTarget, String> {
    capture_expected(app, None).await
}

async fn capture_expected(
    app: &AppHandle,
    expected_pid: Option<i64>,
) -> Result<PasteTarget, String> {
    log::debug!("Target capture requested: expected_pid={expected_pid:?}");
    let value = native::call(
        app,
        json!({"op": "capture", "id": Uuid::new_v4().to_string(), "expected_pid": expected_pid}),
    )
    .await?;
    let target: PasteTarget = serde_json::from_value(value).map_err(|e| e.to_string())?;
    remember(target.clone());
    log::debug!(
        "Target captured: id={} pid={:?} window_id={:?} app={} window={:?} role={:?} via={:?} capability={:?} selection={:?}+{:?}",
        target.id,
        target.process_id,
        target.window_id,
        target.app_name,
        target.window_title,
        target.control_role,
        target.capture_method,
        target.capability,
        target.selection_location,
        target.selection_length
    );
    Ok(target)
}

pub fn begin_dictation_from_hotkey() -> Result<DictationContext, String> {
    begin_dictation(APP.get().ok_or("粘贴服务尚未初始化")?)
}

pub fn record_dictation_capture(target: Option<PasteTarget>, error: Option<String>) {
    if let Some(target) = target {
        remember(target);
    } else if let Ok(mut context) = CONTEXT.lock() {
        context.target_error = error;
    }
}

// Called at the shortcut/command entry, before the recorder task is scheduled.
// Capture starts independently; later phases only await this job's result.
pub fn begin_dictation(app: &AppHandle) -> Result<DictationContext, String> {
    let recording = FOCUS.record()?;
    let origin = native::capture_origin();
    let id = Uuid::new_v4().to_string();
    let mode = db::pastebox_mode()?;
    log::info!("Dictation trigger: job={id} mode={mode:?} origin={origin:?}");
    let app = app.clone();
    let target_id = id.clone();
    Ok(DictationContext::start(id, mode, async move {
        let origin = origin?;
        let pid = origin["pid"]
            .as_i64()
            .filter(|pid| *pid > 0)
            .ok_or("无法确认快捷键触发时的目标应用，内容将保留到粘贴箱")?;
        let request = json!({"op":"capture", "id":target_id, "expected_pid":pid,
                            "expected_input":origin["input"]});
        let value = native::call(&app, request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    })
    .with_recording_lease(recording))
}

pub async fn changed(app: &AppHandle) {
    let count = db::pending_paste_count().unwrap_or(0);
    let _ = native::call(app, json!({"op": "count", "count": count})).await;
    let _ = app.emit("pastebox-changed", ());
}

pub async fn get_state(
    app: &AppHandle,
    before_seq: Option<i64>,
    query: &str,
    pending_only: bool,
) -> Result<PasteboxState, String> {
    let system = native::call(app, json!({"op": "status"})).await?;
    let available = system["available_ids"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let live_targets: Vec<PasteTarget> =
        serde_json::from_value(system["targets"].clone()).unwrap_or_default();
    let update_target = |target: &mut PasteTarget| {
        if let Some(live) = live_targets.iter().find(|live| live.id == target.id) {
            *target = live.clone();
        }
        target.available = available.contains(&json!(target.id));
    };
    let mut items = db::list_paste_items(
        before_seq,
        query.trim().trim_start_matches('#'),
        pending_only,
    )?;
    for item in &mut items {
        if let Some(target) = item.target.as_mut() {
            update_target(target);
        }
    }
    let context = CONTEXT.lock().map_err(|e| e.to_string())?;
    let mut requested_item = context
        .requested_item_id
        .as_deref()
        .and_then(|id| db::get_paste_item(id).ok());
    if let Some(target) = requested_item
        .as_mut()
        .and_then(|item| item.target.as_mut())
    {
        update_target(target);
    }
    let mut targets = context.targets.clone();
    for target in &mut targets {
        update_target(target);
    }
    Ok(PasteboxState {
        notifications_enabled: db::pastebox_notifications_enabled()?,
        items,
        pending_count: db::pending_paste_count()?,
        mode: db::pastebox_mode()?,
        targets,
        opening_target_id: context.opening_target_id.clone(),
        requested_item_id: context.requested_item_id.clone(),
        requested_item,
        target_error: context.target_error.clone(),
        notification_status: system["notification_status"]
            .as_str()
            .unwrap_or("unavailable")
            .into(),
        accessibility_trusted: system["accessibility_trusted"].as_bool().unwrap_or(false),
        backend_error: system["backend_error"].as_str().map(String::from),
    })
}

pub fn close(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn open(
    app: &AppHandle,
    capture_before_open: bool,
    requested: Option<String>,
    position: Option<(f64, f64)>,
) -> Result<(), String> {
    if capture_before_open {
        let result = capture(app).await;
        let mut context = CONTEXT.lock().map_err(|e| e.to_string())?;
        context.opening_target_id = result.as_ref().ok().map(|t| t.id.clone());
        context.target_error = result.err();
    }
    CONTEXT.lock().map_err(|e| e.to_string())?.requested_item_id = requested;
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or("粘贴箱窗口不可用")?;
    if let Some((x, y)) = position {
        window
            .set_position(tauri::LogicalPosition::new(x - 220.0, y + 5.0))
            .map_err(|e| e.to_string())?;
    }
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    let _ = app.emit("pastebox-opened", ());
    Ok(())
}

pub async fn set_mode(app: &AppHandle, mode: PasteMode) -> Result<(), String> {
    db::set_pastebox_mode(mode)?;
    CONTEXT.lock().map_err(|e| e.to_string())?.target_error = None;
    if mode != PasteMode::Manual && db::pastebox_notifications_enabled()? {
        let _ = request_notifications(app).await;
    }
    changed(app).await;
    Ok(())
}

pub async fn set_notifications_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let _lock = NOTIFICATIONS.lock().await;
    db::set_pastebox_notifications_enabled(enabled)?;
    native::call(
        app,
        json!({"op":"notifications_enabled", "enabled":enabled}),
    )
    .await?;
    if enabled {
        let _ = request_notifications(app).await;
    }
    changed(app).await;
    Ok(())
}

pub async fn request_notifications(app: &AppHandle) -> Result<(), String> {
    native::call(app, json!({"op": "permission"})).await?;
    Ok(())
}

pub async fn request_accessibility(app: &AppHandle) -> Result<(), String> {
    native::call(app, json!({"op": "accessibility_permission"})).await?;
    changed(app).await;
    Ok(())
}

pub async fn accessibility_status(app: &AppHandle) -> Result<bool, String> {
    let status = native::call(app, json!({"op": "status"})).await?;
    if let Some(error) = status["backend_error"].as_str() {
        return Err(error.into());
    }
    Ok(status["accessibility_trusted"].as_bool().unwrap_or(false))
}

pub async fn deliver(
    app: &AppHandle,
    id: &str,
    version: i64,
    target_id: Option<String>,
    raw: bool,
    pending_only: bool,
    copy_only: bool,
) -> Result<PasteItem, String> {
    deliver_with_feedback(
        app,
        id,
        version,
        target_id,
        raw,
        pending_only,
        copy_only,
        true,
    )
    .await
}

async fn deliver_with_feedback(
    app: &AppHandle,
    id: &str,
    version: i64,
    target_id: Option<String>,
    raw: bool,
    pending_only: bool,
    copy_only: bool,
    open_on_failure: bool,
) -> Result<PasteItem, String> {
    let automatic = pending_only && open_on_failure;
    // Acquire focus before DELIVERY so a recording cannot hold up unrelated
    // completion notifications. The reservation spans validation and dispatch.
    let (_focus, _lock) = if copy_only {
        (None, DELIVERY.lock().await)
    } else {
        let (focus, lock) = reserve_delivery_focus(automatic).await;
        (Some(focus), lock)
    };
    let (item, lease) = db::claim_paste_item(id, version, pending_only)?;
    let text = if raw { &item.raw_text } else { &item.text };
    let result = if copy_only {
        native::call(app, json!({"op": "copy", "text": text}))
            .await
            .map(|_| ("copied", None))
    } else {
        let target = target_id.or_else(|| item.target.as_ref().map(|t| t.id.clone()));
        if let Some(target) = target {
            // Hiding the panel is not itself target restoration. Native code validates focus.
            let _ = close(app);
            native::call(
                app,
                json!({"op": "paste", "target_id": target, "text": text}),
            )
            .await
            .and_then(|v| serde_json::from_value::<PasteReceipt>(v).map_err(|e| e.to_string()))
            .map(|receipt| {
                if receipt.verified {
                    ("verified", None)
                } else {
                    ("sent", Some(receipt.message))
                }
            })
        } else {
            Err("本条记录没有可恢复的目标，请选择位置或仅复制".into())
        }
    };
    let (status, error) = match result {
        Ok(value) => value,
        Err(error) => ("failed", Some(error)),
    };
    let saved = db::finish_paste_delivery(id, &lease, status, error.as_deref())?;
    let _ = native::call(app, json!({"op": "remove_notification", "id": id})).await;
    changed(app).await;
    if status == "failed" && open_on_failure && !automatic {
        let _ = open(app, false, Some(id.into()), None).await;
    }
    Ok(saved)
}

pub async fn deliver_pending(app: &AppHandle, id: &str) -> Result<PasteItem, String> {
    let item = db::get_paste_item(id)?;
    deliver(app, id, item.version, None, false, true, false).await
}

async fn deliver_from_notification(app: &AppHandle, id: &str) -> Result<PasteItem, String> {
    let item = db::get_paste_item(id)?;
    deliver_with_feedback(app, id, item.version, None, false, true, false, false).await
}

pub async fn process_ready(app: &AppHandle, id: &str) -> Result<PasteItem, String> {
    // Read again after refinement: copying raw text consumes this job's automatic action.
    let item = db::get_paste_item(id)?;
    if item.actioned {
        return Ok(item);
    }
    match item.mode {
        PasteMode::Manual => {}
        PasteMode::Automatic => {
            // Preserve submission order even when later LLM requests finish
            // first. Failed/copy-consumed jobs never block the following item.
            while db::has_earlier_automatic_job(item.seq)? {
                let current = db::get_paste_item(id)?;
                if current.actioned {
                    return Ok(current);
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let _ = deliver_pending(app, id).await;
        }
        PasteMode::Notify => {}
    }
    notifications::completed(app, &db::get_paste_item(id)?).await?;
    changed(app).await;
    db::get_paste_item(id)
}

pub async fn delete(app: &AppHandle, id: &str, version: i64) -> Result<(), String> {
    let _lock = DELIVERY.lock().await;
    db::delete_paste_item(id, version)?;
    if let Ok((audio, normalized)) = crate::voice_input::jobs::paths(id) {
        let _ = std::fs::remove_file(audio);
        let _ = std::fs::remove_file(normalized);
    }
    let _ = native::call(app, json!({"op": "remove_notification", "id": id})).await;
    changed(app).await;
    Ok(())
}

pub async fn clear_history(app: &AppHandle) -> Result<u64, String> {
    let _lock = DELIVERY.lock().await;
    let ids = db::clear_paste_history()?;
    for id in &ids {
        if let Ok((audio, normalized)) = crate::voice_input::jobs::paths(id) {
            let _ = std::fs::remove_file(audio);
            let _ = std::fs::remove_file(normalized);
        }
        let _ = native::call(app, json!({"op": "remove_notification", "id": id})).await;
    }
    changed(app).await;
    Ok(ids.len() as u64)
}

pub fn handle_native_event(event: Value) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        match event["type"].as_str() {
            Some("open") => {
                if app
                    .get_webview_window(WINDOW_LABEL)
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false)
                {
                    let _ = close(&app);
                    return;
                }
                let result = capture_expected(&app, event["expected_pid"].as_i64()).await;
                let (target, error) = (result.as_ref().ok().cloned(), result.err());
                if let Ok(mut context) = CONTEXT.lock() {
                    context.target_error = error.clone();
                    context.opening_target_id = target.as_ref().map(|t| t.id.clone());
                }
                let position = event["x"].as_f64().zip(event["y"].as_f64());
                let _ = open(&app, false, None, position).await;
                changed(&app).await;
            }
            Some("notification") => {
                if let Some(id) = event["id"]
                    .as_str()
                    .filter(|id| Uuid::parse_str(id).is_ok())
                {
                    // No 'latest item' lookup here. Each notification addresses its own ID.
                    if let Err(error) = notifications::clicked(&app, id).await {
                        log::info!("Notification already handled or unavailable: {error}");
                    }
                }
            }
            Some("notification_result") => {
                if let Some(id) = event["id"].as_str() {
                    let error = event["error"].as_str();
                    if let Ok(mut pending) = NOTIFICATION_RESULTS.lock() {
                        if let Some(sender) = pending.remove(id) {
                            let _ =
                                sender.send(error.map(|e| Err(e.to_string())).unwrap_or(Ok(())));
                        }
                    }
                }
            }
            Some("changed") => {
                let _ = app.emit("pastebox-changed", ());
            }
            _ => {}
        }
    });
}
