use crate::audio::{default_output_device, supports_system_audio};
use crate::recording::RECORDING_STATE;
use crate::types::{RecordingArgs, RecordingInfo};
use tauri::{AppHandle, Runtime};

#[tauri::command]
pub async fn start_recording<R: Runtime>(_app: AppHandle<R>) -> Result<(), String> {
    crate::recording::controller::start_recording().await
}

#[tauri::command]
pub async fn stop_recording(args: RecordingArgs) -> Result<(), String> {
    crate::recording::controller::stop_recording(args).await
}

#[tauri::command]
pub fn is_recording() -> bool {
    crate::recording::is_recording()
}

#[tauri::command]
pub fn get_recording_info() -> RecordingInfo {
    use crate::recording::{RecordingMode, IS_RECORDING};
    use std::sync::atomic::Ordering;

    if let Ok(state) = RECORDING_STATE.lock() {
        let is_recording = IS_RECORDING.load(Ordering::SeqCst);
        let duration = if is_recording {
            if let Some(start_time) = state.start_time {
                start_time.elapsed().as_secs_f64()
            } else {
                0.0
            }
        } else {
            0.0
        };

        let recording_mode = match state.recording_mode {
            RecordingMode::MixedAudio => "Mixed Audio (Mic + System)".to_string(),
            RecordingMode::MicrophoneOnly => "Microphone Only".to_string(),
        };

        let system_audio_available = match default_output_device() {
            Ok(device) => supports_system_audio(&device),
            Err(_) => false,
        };

        RecordingInfo {
            is_recording,
            duration,
            path: None,
            recording_mode,
            system_audio_available,
        }
    } else {
        RecordingInfo {
            is_recording: false,
            duration: 0.0,
            path: None,
            recording_mode: "Unknown".to_string(),
            system_audio_available: false,
        }
    }
}
