use anyhow::Result;
use log::{debug, info, warn};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio::{
    default_output_device, AudioStream, StreamingResampler, RESAMPLER_CHUNK_SIZE,
    TARGET_SAMPLE_RATE,
};
use crate::recording::buffer::{flush_buffers_to_temp_files, should_flush_buffer};
use crate::recording::state::{is_recording, RECORDING_STATE};

/// Start mixed audio recording (microphone + system audio)
pub async fn start_mixed_recording() -> Result<()> {
    info!("Starting mixed audio recording...");

    // Get input and output devices
    let mic_device = Arc::new(
        crate::audio::default_input_device()
            .map_err(|e| anyhow::anyhow!("Failed to get default input device: {}", e))?,
    );

    let system_device = Arc::new(
        default_output_device()
            .map_err(|e| anyhow::anyhow!("Failed to get default output device: {}", e))?,
    );

    info!(
        "Using mic device: {} and system device: {}",
        mic_device.name, system_device.name
    );

    // Create individual audio streams
    let is_recording_arc = Arc::new(AtomicBool::new(true));

    let mic_stream = AudioStream::from_device(mic_device.clone(), is_recording_arc.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create mic stream: {}", e))?;

    let system_stream = AudioStream::from_device(system_device.clone(), is_recording_arc.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create system stream: {}", e))?;

    let mic_stream = Arc::new(mic_stream);
    let system_stream = Arc::new(system_stream);

    // Get native device sample rates
    let mic_native_rate = mic_stream.device_config.sample_rate().0;
    let system_native_rate = system_stream.device_config.sample_rate().0;

    info!(
        "Native device sample rates - Mic: {} Hz, System: {} Hz",
        mic_native_rate, system_native_rate
    );
    info!(
        "Target downsampling rate: {} Hz (optimized for file size)",
        TARGET_SAMPLE_RATE
    );

    // Create streaming resamplers for real-time processing
    let mic_resampler =
        StreamingResampler::new(mic_native_rate, TARGET_SAMPLE_RATE, RESAMPLER_CHUNK_SIZE)
            .map_err(|e| anyhow::anyhow!("Failed to create mic resampler: {}", e))?;
    let system_resampler =
        StreamingResampler::new(system_native_rate, TARGET_SAMPLE_RATE, RESAMPLER_CHUNK_SIZE)
            .map_err(|e| anyhow::anyhow!("Failed to create system resampler: {}", e))?;

    // Store streams and resamplers in state for cleanup
    {
        let mut state = RECORDING_STATE.lock().unwrap();
        state.target_sample_rate = TARGET_SAMPLE_RATE;
        state.mic_native_rate = mic_native_rate;
        state.system_native_rate = system_native_rate;
        state.stream = Some(mic_stream.clone());
        state.system_stream = Some(system_stream.clone());
        state.mic_resampler = Some(Arc::new(Mutex::new(mic_resampler)));
        state.system_resampler = Some(Arc::new(Mutex::new(system_resampler)));
    }

    // Subscribe to both audio streams
    let mut mic_receiver = mic_stream.subscribe().await;
    let mut system_receiver = system_stream.subscribe().await;

    info!(
        "Dual audio streams started, native rates - Mic: {} Hz, System: {} Hz",
        mic_native_rate, system_native_rate
    );
    info!(
        "Real-time downsampling to: {} Hz for optimal file size",
        TARGET_SAMPLE_RATE
    );

    // Collect and process audio data using dual buffer pattern with streaming resamplers
    while is_recording() {
        let mut mic_samples = Vec::new();
        let mut system_samples = Vec::new();

        // Get microphone samples (non-blocking)
        while let Ok(chunk) = mic_receiver.try_recv() {
            mic_samples.extend(chunk);
        }

        // Get system audio samples (non-blocking)
        while let Ok(chunk) = system_receiver.try_recv() {
            system_samples.extend(chunk);
        }

        // Process samples with streaming resamplers (real-time downsampling)
        let (mic_processed, system_processed) = {
            let state = RECORDING_STATE.lock().unwrap();

            let mic_processed = if !mic_samples.is_empty() {
                if let Some(resampler_arc) = &state.mic_resampler {
                    if let Ok(mut resampler) = resampler_arc.lock() {
                        match resampler.process_chunk(&mic_samples) {
                            Ok(processed) => {
                                if !processed.is_empty() {
                                    debug!(
                                        "Mic resampled: {} -> {} samples",
                                        mic_samples.len(),
                                        processed.len()
                                    );
                                }
                                processed
                            }
                            Err(e) => {
                                warn!("Mic resampling failed: {}, using original samples", e);
                                mic_samples.clone()
                            }
                        }
                    } else {
                        mic_samples.clone()
                    }
                } else {
                    mic_samples.clone()
                }
            } else {
                Vec::new()
            };

            let system_processed = if !system_samples.is_empty() {
                if let Some(resampler_arc) = &state.system_resampler {
                    if let Ok(mut resampler) = resampler_arc.lock() {
                        match resampler.process_chunk(&system_samples) {
                            Ok(processed) => {
                                if !processed.is_empty() {
                                    debug!(
                                        "System resampled: {} -> {} samples",
                                        system_samples.len(),
                                        processed.len()
                                    );
                                }
                                processed
                            }
                            Err(e) => {
                                warn!("System resampling failed: {}, using original samples", e);
                                system_samples.clone()
                            }
                        }
                    } else {
                        system_samples.clone()
                    }
                } else {
                    system_samples.clone()
                }
            } else {
                Vec::new()
            };

            (mic_processed, system_processed)
        };

        // Store processed samples in dual buffers and check for periodic flush
        let (should_flush, flush_info) = {
            let state = RECORDING_STATE.lock().unwrap();
            let mut mic_buffer_len = 0;
            let mut system_buffer_len = 0;

            if let Some(buffer) = &state.mic_buffer {
                if let Ok(mut guard) = buffer.lock() {
                    guard.extend_from_slice(&mic_processed);
                    mic_buffer_len = guard.len();
                }
            }

            if let Some(buffer) = &state.system_buffer {
                if let Ok(mut guard) = buffer.lock() {
                    guard.extend_from_slice(&system_processed);
                    system_buffer_len = guard.len();
                }
            }

            // Check if we should flush buffers to temp files
            let should_flush =
                should_flush_buffer(mic_buffer_len.max(system_buffer_len), state.last_flush_time);
            let flush_info = if should_flush && mic_buffer_len > 0 {
                if let (Some(temp_dir), segment_index) = (&state.temp_dir, state.segment_index) {
                    Some((temp_dir.clone(), segment_index))
                } else {
                    None
                }
            } else {
                None
            };

            (should_flush, flush_info)
        };

        // Perform flush operation outside the lock
        if should_flush {
            if let Some((temp_dir, segment_index)) = flush_info {
                if let Err(e) = flush_buffers_to_temp_files(temp_dir, segment_index).await {
                    warn!("Failed to flush buffers to temp files: {}", e);
                } else {
                    // Update state after successful flush
                    if let Ok(mut state) = RECORDING_STATE.lock() {
                        state.segment_index += 1;
                        state.last_flush_time = Some(Instant::now());
                    }
                }
            }
        }

        // Small sleep to prevent busy waiting
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    info!("Mixed recording data collection finished");
    Ok(())
}
