use anyhow::Result;
use log::{debug, info, warn};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

use crate::audio::{AudioStream, StreamingResampler, RESAMPLER_CHUNK_SIZE, TARGET_SAMPLE_RATE};
use crate::recording::buffer::{flush_mic_buffer_to_temp_file, should_flush_buffer};
use crate::recording::state::{is_recording, set_recording, RECORDING_STATE};

/// Start microphone-only recording
pub async fn start_microphone_only_recording() -> Result<()> {
    info!("Starting microphone-only recording...");

    // Get default input device
    let device = crate::audio::default_input_device()
        .map_err(|e| anyhow::anyhow!("Failed to get default input device: {}", e))?;

    info!("Using input device: {}", device.name);

    // Create audio stream
    let is_recording_arc = Arc::new(AtomicBool::new(true));
    set_recording(true);
    let stream = AudioStream::from_device(Arc::new(device), is_recording_arc.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create audio stream: {}", e))?;

    let mic_native_rate = stream.device_config.sample_rate().0;
    info!(
        "Microphone native sample rate: {} Hz, target rate: {} Hz",
        mic_native_rate, TARGET_SAMPLE_RATE
    );

    // Create streaming resampler for microphone-only mode
    let mic_resampler =
        StreamingResampler::new(mic_native_rate, TARGET_SAMPLE_RATE, RESAMPLER_CHUNK_SIZE)
            .map_err(|e| anyhow::anyhow!("Failed to create mic resampler: {}", e))?;

    // Store stream and resampler in state
    {
        let mut state = RECORDING_STATE.lock().unwrap();
        state.target_sample_rate = TARGET_SAMPLE_RATE;
        state.mic_native_rate = mic_native_rate;
        state.stream = Some(Arc::new(stream));
        state.mic_resampler = Some(Arc::new(Mutex::new(mic_resampler)));
    }

    // Subscribe to audio data
    let mut receiver = {
        let stream_arc = {
            let state = RECORDING_STATE.lock().unwrap();
            if let Some(ref stream) = state.stream {
                stream.clone()
            } else {
                return Err(anyhow::anyhow!("Stream not initialized"));
            }
        };
        stream_arc.subscribe().await
    };

    info!(
        "Audio stream started, collecting and processing data with native rate: {} Hz",
        mic_native_rate
    );

    // Collect audio data with real-time resampling using streaming resampler
    while is_recording() {
        match tokio::time::timeout(Duration::from_millis(100), receiver.recv()).await {
            Ok(Ok(audio_chunk)) => {
                // Process chunk with streaming resampler (real-time downsampling)
                let processed_chunk = if !audio_chunk.is_empty() {
                    let state = RECORDING_STATE.lock().unwrap();
                    if let Some(resampler_arc) = &state.mic_resampler {
                        if let Ok(mut resampler) = resampler_arc.lock() {
                            match resampler.process_chunk(&audio_chunk) {
                                Ok(processed) => {
                                    if !processed.is_empty() {
                                        debug!(
                                            "Mic processed: {} -> {} samples ({}Hz -> {}Hz)",
                                            audio_chunk.len(),
                                            processed.len(),
                                            mic_native_rate,
                                            TARGET_SAMPLE_RATE
                                        );
                                    }
                                    processed
                                }
                                Err(e) => {
                                    warn!("Mic resampling failed: {}, using original samples", e);
                                    audio_chunk
                                }
                            }
                        } else {
                            audio_chunk
                        }
                    } else {
                        audio_chunk
                    }
                } else {
                    audio_chunk
                };

                // Store processed data in mic buffer and check for periodic flush
                let (should_flush, flush_info) = {
                    let state = RECORDING_STATE.lock().unwrap();
                    let mut mic_buffer_len = 0;

                    if let Some(buffer) = &state.mic_buffer {
                        if let Ok(mut guard) = buffer.lock() {
                            guard.extend_from_slice(&processed_chunk);
                            mic_buffer_len = guard.len();
                        }
                    }

                    // Check if we should flush buffer to temp file
                    let should_flush = should_flush_buffer(mic_buffer_len, state.last_flush_time);
                    let flush_info = if should_flush && mic_buffer_len > 0 {
                        if let (Some(temp_dir), segment_index) =
                            (&state.temp_dir, state.segment_index)
                        {
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
                        if let Err(e) = flush_mic_buffer_to_temp_file(temp_dir, segment_index).await
                        {
                            warn!("Failed to flush mic buffer to temp file: {}", e);
                        } else {
                            // Update state after successful flush
                            if let Ok(mut state) = RECORDING_STATE.lock() {
                                state.segment_index += 1;
                                state.last_flush_time = Some(Instant::now());
                            }
                        }
                    }
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                warn!("Audio receiver lagged, continuing...");
                continue;
            }
            Ok(Err(_)) => {
                warn!("Audio receiver error, stopping...");
                break;
            }
            Err(_) => {
                // Timeout, continue to check recording flag
                continue;
            }
        }
    }

    info!("Recording data collection finished");
    Ok(())
}
