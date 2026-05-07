use std::time::Duration;

use tauri::{AppHandle, Manager};

const VOICE_INPUT_OVERLAY_LABEL: &str = "voice-input-overlay";
const VOICE_INPUT_OVERLAY_WIDTH: f64 = 256.0;
const VOICE_INPUT_OVERLAY_HEIGHT: f64 = 32.0;
#[cfg(test)]
const VOICE_INPUT_OVERLAY_BOTTOM_MARGIN: i32 = 34;
const VOICE_INPUT_FINAL_HIDE_DELAY_MS: u64 = 3_200;

pub(super) fn update_voice_input_overlay(app: &AppHandle, phase: &str) {
    if is_overlay_visible_phase(phase) {
        show_voice_input_overlay(app);
    } else {
        hide_voice_input_overlay(app);
    }

    if is_overlay_final_phase(phase) {
        schedule_voice_input_overlay_hide(app.clone());
    }
}

fn is_overlay_visible_phase(phase: &str) -> bool {
    matches!(
        phase,
        "starting"
            | "listening"
            | "preparing_model"
            | "transcribing"
            | "refining"
            | "inserting"
            | "inserted"
            | "copied"
            | "failed"
            | "cancelled"
    )
}

fn is_overlay_final_phase(phase: &str) -> bool {
    matches!(phase, "inserted" | "copied" | "failed" | "cancelled")
}

fn show_voice_input_overlay(app: &AppHandle) {
    let Some(window) = app.get_webview_window(VOICE_INPUT_OVERLAY_LABEL) else {
        return;
    };
    if let Err(error) = window.set_size(tauri::LogicalSize::new(
        VOICE_INPUT_OVERLAY_WIDTH,
        VOICE_INPUT_OVERLAY_HEIGHT,
    )) {
        log::warn!("Failed to size voice input overlay: {error}");
    }
    if let Err(error) = window.set_always_on_top(true) {
        log::warn!("Failed to keep voice input overlay on top: {error}");
    }
    if let Err(error) = window.set_visible_on_all_workspaces(true) {
        log::warn!("Failed to show voice input overlay on all workspaces: {error}");
    }
    if let Err(error) = window.set_focusable(false) {
        log::warn!("Failed to keep voice input overlay non-focusable: {error}");
    }
    if let Err(error) = window.show() {
        log::warn!("Failed to show voice input overlay: {error}");
    }
}

fn hide_voice_input_overlay(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(VOICE_INPUT_OVERLAY_LABEL) {
        if let Err(error) = window.hide() {
            log::warn!("Failed to hide voice input overlay: {error}");
        }
    }
}

fn schedule_voice_input_overlay_hide(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(VOICE_INPUT_FINAL_HIDE_DELAY_MS)).await;
        if super::is_idle_phase() {
            hide_voice_input_overlay(&app);
        }
    });
}

#[cfg(test)]
pub(crate) fn overlay_position_for_work_area(
    work_x: i32,
    work_y: i32,
    work_width: u32,
    work_height: u32,
    window_width: u32,
    window_height: u32,
) -> (i32, i32) {
    let x = work_x + ((work_width as i32 - window_width as i32) / 2).max(0);
    let y = work_y + work_height as i32 - window_height as i32 - VOICE_INPUT_OVERLAY_BOTTOM_MARGIN;
    (x, y.max(work_y))
}
