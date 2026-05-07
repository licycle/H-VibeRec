use anyhow::Result;
use log::{error, info, warn};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio::{default_output_device, supports_system_audio, TARGET_SAMPLE_RATE};
use crate::recording::microphone::start_microphone_only_recording;
use crate::recording::mixed::start_mixed_recording;
use crate::recording::state::{is_recording, set_recording, RecordingMode, RECORDING_STATE};
use crate::storage::{get_temp_dir, stream_merge_temp_files_to_wav, write_buffer_to_temp_file};
use crate::types::RecordingArgs;

/// Initialize and start recording
pub async fn start_recording() -> Result<(), String> {
    info!("Starting recording command...");

    // Check if already recording
    if is_recording() {
        return Err("Recording already in progress".to_string());
    }

    crate::audio::trigger_audio_permission()
        .map_err(|e| format!("Microphone permission check failed: {}", e))?;

    let recording_mode = determine_recording_mode().await;
    let temp_dir = get_temp_dir().map_err(|e| format!("Failed to create temp dir: {}", e))?;

    // Initialize recording state with dual-stream setup
    {
        let mut state = RECORDING_STATE
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        state.start_time = Some(Instant::now());
        state.target_sample_rate = TARGET_SAMPLE_RATE;
        state.stream = None;
        state.system_stream = None;
        state.mic_buffer = Some(Arc::new(Mutex::new(Vec::new())));
        state.system_buffer = Some(Arc::new(Mutex::new(Vec::new())));
        state.mic_resampler = None;
        state.system_resampler = None;
        state.mic_native_rate = 0;
        state.system_native_rate = 0;

        // Initialize streaming file processing
        state.segment_index = 0;
        state.temp_dir = Some(temp_dir);
        state.last_flush_time = Some(Instant::now());
        state.recording_mode = recording_mode;
    }

    // Set recording flag
    set_recording(true);

    // Start recording in a separate task
    tokio::spawn(async {
        if let Err(e) = start_recording_internal().await {
            error!("Recording failed: {}", e);
            // Reset recording state on error
            set_recording(false);
            if let Ok(mut state) = RECORDING_STATE.lock() {
                state.reset();
            }
        }
    });

    Ok(())
}

async fn determine_recording_mode() -> RecordingMode {
    match default_output_device() {
        Ok(system_device) if supports_system_audio(&system_device) => {
            #[cfg(target_os = "macos")]
            {
                match crate::audio::trigger_system_audio_permission().await {
                    Ok(()) => {
                        info!(
                            "System audio capture available: {}, enabling mixed recording",
                            system_device.name
                        );
                        RecordingMode::MixedAudio
                    }
                    Err(e) => {
                        warn!(
                            "System audio capture is not authorized or unavailable ({}), using microphone only",
                            e
                        );
                        RecordingMode::MicrophoneOnly
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                info!(
                    "System audio device available: {}, enabling mixed recording",
                    system_device.name
                );
                RecordingMode::MixedAudio
            }
        }
        Ok(system_device) => {
            warn!(
                "System audio device found but doesn't support capture: {}, using microphone only",
                system_device.name
            );
            RecordingMode::MicrophoneOnly
        }
        Err(e) => {
            warn!(
                "No system audio device available ({}), using microphone only",
                e
            );
            RecordingMode::MicrophoneOnly
        }
    }
}

async fn start_recording_internal() -> Result<()> {
    info!("Starting recording internal...");

    let recording_mode = {
        let state = RECORDING_STATE.lock().unwrap();
        state.recording_mode.clone()
    };

    match recording_mode {
        RecordingMode::MixedAudio => start_mixed_recording().await,
        RecordingMode::MicrophoneOnly => start_microphone_only_recording().await,
    }
}

/// Stop recording and save to file
pub async fn stop_recording(args: RecordingArgs) -> Result<(), String> {
    info!("Stopping recording command...");

    // Check if recording is in progress
    if !is_recording() {
        info!("No recording in progress");
        return Ok(());
    }

    // Signal recording to stop
    set_recording(false);
    info!("Recording flag set to false");

    // Stop audio streams
    let (mic_stream_to_stop, system_stream_to_stop) = {
        let mut state = RECORDING_STATE
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        (state.stream.take(), state.system_stream.take())
    };

    // Stop mic stream
    if let Some(stream) = mic_stream_to_stop {
        if let Err(e) = stream.stop().await {
            warn!("Failed to stop mic stream: {}", e);
        }
    }

    // Stop system stream
    if let Some(stream) = system_stream_to_stop {
        if let Err(e) = stream.stop().await {
            warn!("Failed to stop system stream: {}", e);
        }
    }

    // Wait 1 second + 100ms buffer for any remaining data and flush resamplers
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // Flush any remaining samples from streaming resamplers
    {
        let state = RECORDING_STATE
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;

        if let Some(resampler_arc) = &state.mic_resampler {
            if let Ok(mut resampler) = resampler_arc.lock() {
                match resampler.flush() {
                    Ok(remaining) => {
                        if !remaining.is_empty() {
                            info!(
                                "Flushed {} remaining mic samples from resampler",
                                remaining.len()
                            );
                            if let Some(buffer) = &state.mic_buffer {
                                if let Ok(mut guard) = buffer.lock() {
                                    guard.extend_from_slice(&remaining);
                                }
                            }
                        }
                    }
                    Err(e) => warn!("Failed to flush mic resampler: {}", e),
                }
            }
        }

        if let Some(resampler_arc) = &state.system_resampler {
            if let Ok(mut resampler) = resampler_arc.lock() {
                match resampler.flush() {
                    Ok(remaining) => {
                        if !remaining.is_empty() {
                            info!(
                                "Flushed {} remaining system samples from resampler",
                                remaining.len()
                            );
                            if let Some(buffer) = &state.system_buffer {
                                if let Ok(mut guard) = buffer.lock() {
                                    guard.extend_from_slice(&remaining);
                                }
                            }
                        }
                    }
                    Err(e) => warn!("Failed to flush system resampler: {}", e),
                }
            }
        }
    }

    // Process final buffer data and prepare for streaming merge
    let (
        segment_count,
        temp_dir,
        recording_mode,
        recording_start_time,
        final_mic_data,
        final_system_data,
    ) = {
        let mut state = RECORDING_STATE
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;

        // Write final buffer data to temp files if any
        let mut final_mic_data = Vec::new();
        let mut final_system_data = Vec::new();

        if let Some(buffer) = &state.mic_buffer {
            if let Ok(guard) = buffer.lock() {
                final_mic_data = guard.clone();
                info!("Final mic buffer contains {} samples", final_mic_data.len());
            }
        }

        if let Some(buffer) = &state.system_buffer {
            if let Ok(guard) = buffer.lock() {
                final_system_data = guard.clone();
                info!(
                    "Final system buffer contains {} samples",
                    final_system_data.len()
                );
            }
        }

        let recording_mode = state.recording_mode.clone();
        let recording_start_time = state.start_time;
        let segment_count = state.segment_index;
        let temp_dir = state.temp_dir.clone();

        // Clear state but keep temp_dir reference
        state.reset();

        (
            segment_count,
            temp_dir,
            recording_mode,
            recording_start_time,
            final_mic_data,
            final_system_data,
        )
    };

    // Write final buffer data to temp files if we have data
    let final_segment_count = if let Some(temp_dir_ref) = &temp_dir {
        if !final_mic_data.is_empty() || !final_system_data.is_empty() {
            info!(
                "Writing final segment {} with {} mic and {} system samples",
                segment_count,
                final_mic_data.len(),
                final_system_data.len()
            );

            if !final_mic_data.is_empty() {
                write_buffer_to_temp_file(&final_mic_data, segment_count, "mic", temp_dir_ref)?;
            }
            if !final_system_data.is_empty() {
                write_buffer_to_temp_file(
                    &final_system_data,
                    segment_count,
                    "system",
                    temp_dir_ref,
                )?;
            }
            segment_count + 1
        } else {
            segment_count
        }
    } else {
        segment_count
    };

    // Time duration validation
    let recording_duration_ms = if let Some(start_time) = recording_start_time {
        start_time.elapsed().as_millis() as f64
    } else {
        0.0
    };

    info!("Recording duration: {:.2}s", recording_duration_ms / 1000.0);
    info!("Total segments to process: {}", final_segment_count);

    // Check if we have any data to process
    if final_segment_count == 0 {
        return Err("No audio data captured".to_string());
    }

    // Create save directory
    if let Some(parent) = std::path::Path::new(&args.save_path).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create save directory: {}", e))?;
        }
    }

    // Use streaming merge to create final WAV file
    if let Some(temp_dir_ref) = temp_dir {
        info!(
            "Starting streaming merge of {} segments",
            final_segment_count
        );
        stream_merge_temp_files_to_wav(
            final_segment_count,
            &temp_dir_ref,
            &args.save_path,
            &recording_mode,
        )
        .await?;

        info!(
            "Successfully completed streaming merge to: {}",
            args.save_path
        );

        // Clean up temp directory
        if let Err(e) = std::fs::remove_dir_all(&temp_dir_ref) {
            warn!("Failed to remove temp directory: {}", e);
        } else {
            info!("Cleaned up temp directory: {}", temp_dir_ref.display());
        }
    } else {
        return Err("No temp directory available for processing".to_string());
    }

    info!(
        "Successfully saved streaming FLAC recording to: {}",
        args.save_path
    );
    Ok(())
}
