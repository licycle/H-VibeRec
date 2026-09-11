use super::{changed, close, db, native, DELIVERY, NOTIFICATIONS, NOTIFICATION_RESULTS};
use crate::types::{PasteItem, PasteMode};
use serde_json::json;
use tauri::AppHandle;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ClickAction {
    CopyAndRestore,
    Ignore,
}

pub(super) fn click_action(item: &PasteItem) -> ClickAction {
    match item.mode {
        PasteMode::Automatic => ClickAction::CopyAndRestore,
        PasteMode::Notify
            if !item.actioned
                && item.processing_status == "ready"
                && item.delivery_status == "pending" =>
        {
            ClickAction::CopyAndRestore
        }
        _ => ClickAction::Ignore,
    }
}

pub(super) async fn completed(app: &AppHandle, item: &PasteItem) -> Result<(), String> {
    // Keep claims/deletions from overtaking the asynchronous OS enqueue.
    // Delivery always removes its banner after this lock is released.
    let _delivery = DELIVERY.lock().await;
    let _lock = NOTIFICATIONS.lock().await;
    if item.mode == PasteMode::Manual || !db::pastebox_notifications_enabled()? {
        return Ok(());
    }
    // Read current state so a manual action racing refinement cannot get an old
    // banner. Every notification now has the same copy-and-return action.
    let item = db::get_paste_item(&item.id)?;
    let action = match click_action(&item) {
        ClickAction::CopyAndRestore => "restore",
        ClickAction::Ignore => return Ok(()),
    };
    let title = match item.delivery_status.as_str() {
        "verified" => "语音结果已就绪",
        "sent" | "uncertain" => "语音结果已就绪，请检查原窗口",
        "failed" => "语音已完成，可复制结果",
        _ => "语音结果已就绪",
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    NOTIFICATION_RESULTS
        .lock()
        .map_err(|e| e.to_string())?
        .insert(item.id.clone(), sender);
    let result = match native::call(
        app,
        json!({"op":"notify", "id":item.id, "seq":item.seq,
        "action":action, "title":title,
        "target_name":item.target.as_ref().map(|t| t.app_name.as_str()).unwrap_or("原位置"),
        "preview":item.text.chars().take(90).collect::<String>()}),
    )
    .await
    {
        Err(error) => Err(error),
        Ok(_) => match tokio::time::timeout(std::time::Duration::from_secs(5), receiver).await {
            Ok(Ok(result)) => result,
            _ => Err("系统未确认通知，内容已保留到粘贴箱".into()),
        },
    };
    if let Ok(mut pending) = NOTIFICATION_RESULTS.lock() {
        pending.remove(&item.id);
    }
    db::set_paste_notification_error(&item.id, result.err().as_deref())?;
    changed(app).await;
    Ok(())
}

async fn perform_click(
    app: &AppHandle,
    item: &PasteItem,
) -> Result<(String, Option<String>), String> {
    match click_action(item) {
        ClickAction::CopyAndRestore => {
            let target = item
                .target
                .as_ref()
                .ok_or("本条听写未记录可恢复的位置，内容已保留")?;

            // A semi-automatic notification consumes its pending item as
            // copied, but never dispatches Cmd+V. Automatic items are already
            // actioned by the FIFO worker and only need their text copied.
            if item.mode == PasteMode::Notify && !item.actioned {
                super::deliver_with_feedback(
                    app,
                    &item.id,
                    item.version,
                    None,
                    false,
                    true,
                    true,
                    false,
                )
                .await?;
            } else {
                native::call(app, json!({"op":"copy", "text":item.text})).await?;
            }

            // Copy first, then return to the exact recorded window/caret. The
            // result deliberately remains in the system clipboard for the
            // user to paste again if the target application is AX-opaque.
            let (_focus, _lock) = super::reserve_delivery_focus(false).await;
            let _ = close(app);
            let receipt = native::call(app, json!({"op":"restore", "target_id":target.id})).await?;
            if receipt["restored"] != true || !receipt["caret_restored"].is_boolean() {
                return Err("目标返回操作未提供可确认的结果".into());
            }
            let message = if receipt["caret_restored"] == true {
                "已复制到剪贴板，并返回对应光标位置".to_string()
            } else {
                format!(
                    "已复制到剪贴板，并返回原窗口：{}",
                    receipt["message"].as_str().unwrap_or("光标位置未确认")
                )
            };
            let detail = (receipt["caret_restored"] != true).then(|| {
                receipt["reason"]
                    .as_str()
                    .unwrap_or("无法确认原光标")
                    .to_string()
            });
            Ok((message, detail))
        }
        ClickAction::Ignore => Ok(("本条已处理，不会重复粘贴".into(), None)),
    }
}

pub(super) async fn clicked(app: &AppHandle, id: &str) -> Result<(), String> {
    let item = db::get_paste_item(id)?;
    let result = perform_click(app, &item).await;
    let (message, detail) = match &result {
        Ok((message, detail)) => (message.clone(), detail.clone()),
        Err(error) => (format!("无法返回目标：{error}"), Some(error.clone())),
    };
    // Record feedback for the exact item. Opening the panel here would steal
    // focus back from a window that was successfully restored.
    let _ = db::set_paste_notification_error(id, detail.as_deref());
    if let Ok(mut context) = super::CONTEXT.lock() {
        context.target_error = detail.clone();
    }
    log::info!("Notification action: id={id} seq={} action={:?} delivery={} result={message} detail={detail:?}",
        item.seq, click_action(&item), item.delivery_status);
    crate::voice_input::show_navigation_feedback(app, &format!("#{} {message}", item.seq));
    changed(app).await;
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(mode: PasteMode, status: &str, actioned: bool) -> PasteItem {
        serde_json::from_value(json!({"id":"test", "seq":1,"text":"test","raw_text":"test",
            "processing_status":"ready","delivery_status":status,"mode":mode,"target":null,
            "created_at":"", "version":0,"actioned":actioned}))
        .unwrap()
    }
    #[test]
    fn automatic_notifications_never_request_paste_even_after_failure() {
        for status in [
            "pending",
            "verified",
            "sent",
            "failed",
            "copied",
            "uncertain",
        ] {
            for actioned in [false, true] {
                assert_eq!(
                    click_action(&item(PasteMode::Automatic, status, actioned)),
                    ClickAction::CopyAndRestore
                );
            }
        }
    }
    #[test]
    fn notify_click_only_consumes_its_pending_item() {
        assert_eq!(
            click_action(&item(PasteMode::Notify, "pending", false)),
            ClickAction::CopyAndRestore
        );
        for status in ["pending", "verified", "failed", "sent", "copied"] {
            assert_eq!(
                click_action(&item(PasteMode::Notify, status, true)),
                ClickAction::Ignore
            );
        }
        assert_eq!(
            click_action(&item(PasteMode::Manual, "pending", false)),
            ClickAction::Ignore
        );
    }
}
