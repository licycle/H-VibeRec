use serde_json::Value;
use tauri::AppHandle;

// Cocoa owns shortcut origins, menu bar, hotkeys, notifications and clipboard.
// PyObjC owns AX target references and text selections.
#[cfg(target_os = "macos")]
mod macos {
    use serde_json::Value;
    use std::ffi::{c_char, c_void, CStr, CString};
    use tauri::{AppHandle, Manager};

    extern "C" {
        fn hvr_pastebox_init(callback: unsafe extern "C" fn(*const c_char));
        fn hvr_pastebox_allow_target_window(window: *mut c_void);
        fn hvr_pastebox_call(input: *const c_char) -> *mut c_char;
        fn hvr_pastebox_capture_origin() -> *mut c_char;
        fn hvr_pastebox_free(value: *mut c_char);
    }

    unsafe extern "C" fn callback(value: *const c_char) {
        if value.is_null() {
            return;
        }
        if let Ok(event) = serde_json::from_slice(CStr::from_ptr(value).to_bytes()) {
            super::super::handle_native_event(event);
        }
    }

    pub fn init(app: &AppHandle) {
        unsafe { hvr_pastebox_init(callback) };
        // Mark the exact native main window, not a title that can be duplicated.
        // The helper can capture our editor while excluding the overlay/outbox.
        if let Some(main) = app.get_webview_window("main") {
            match main.ns_window() {
                Ok(window) => unsafe { hvr_pastebox_allow_target_window(window) },
                Err(error) => log::warn!("Cannot identify the app's editable window: {error}"),
            }
        }
    }

    pub fn capture_origin() -> Result<Value, String> {
        decode(unsafe { hvr_pastebox_capture_origin() })
    }

    pub fn call(request: &Value) -> Result<Value, String> {
        let input = CString::new(request.to_string()).map_err(|e| e.to_string())?;
        decode(unsafe { hvr_pastebox_call(input.as_ptr()) })
    }

    fn decode(output: *mut c_char) -> Result<Value, String> {
        unsafe {
            if output.is_null() {
                return Err("系统操作未返回结果".into());
            }
            let result = serde_json::from_slice::<Value>(CStr::from_ptr(output).to_bytes());
            hvr_pastebox_free(output);
            let result = result.map_err(|e| e.to_string())?;
            if let Some(error) = result.get("error").and_then(Value::as_str) {
                return Err(error.into());
            }
            Ok(result)
        }
    }
}

pub fn capture_origin() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        macos::capture_origin()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("记录输入位置当前仅支持 macOS".into())
    }
}

pub fn init(_app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos::init(_app);
}

pub async fn call(app: &AppHandle, mut request: Value) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    match request["op"].as_str() {
        Some("capture") => {
            if !request["expected_pid"].is_i64() {
                request["expected_pid"] =
                    cocoa_call(app, serde_json::json!({"op": "frontmost"})).await?["pid"].clone();
            }
            return super::worker::call(app, request).await;
        }
        Some("clone" | "paste" | "restore" | "accessibility_permission") => {
            return super::worker::call(app, request).await
        }
        Some("status") => {
            let mut status = cocoa_call(app, request.clone()).await?;
            match super::worker::call(app, request).await {
                Ok(ax) => {
                    for key in [
                        "available_ids",
                        "targets",
                        "accessibility_trusted",
                        "engine",
                    ] {
                        status[key] = ax[key].clone();
                    }
                }
                Err(error) => {
                    status["available_ids"] = serde_json::json!([]);
                    status["accessibility_trusted"] = serde_json::json!(false);
                    status["backend_error"] = serde_json::json!(error);
                }
            }
            return Ok(status);
        }
        _ => {}
    }
    cocoa_call(app, request).await
}

async fn cocoa_call(app: &AppHandle, request: Value) -> Result<Value, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        let result = macos::call(&request);
        #[cfg(not(target_os = "macos"))]
        let result = match request["op"].as_str() {
            Some("status") => {
                Ok(serde_json::json!({"notification_status": "unavailable", "available_ids": []}))
            }
            Some("count") | Some("remove_notification") => Ok(serde_json::json!({})),
            _ => Err("系统粘贴与通知当前仅支持 macOS".into()),
        };
        let _ = sender.send(result);
    })
    .map_err(|e| e.to_string())?;
    receiver.await.map_err(|e| e.to_string())?
}
