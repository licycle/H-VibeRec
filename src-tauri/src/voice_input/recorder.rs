use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};
use log::{info, warn};
use tokio::sync::broadcast;

use crate::audio::{AudioStream, StreamingResampler, RESAMPLER_CHUNK_SIZE, TARGET_SAMPLE_RATE};

pub struct ActiveShortRecorder {
    running: Arc<AtomicBool>,
    stream: Arc<AudioStream>,
    samples: Arc<Mutex<Vec<f32>>>,
    resampler: Arc<Mutex<StreamingResampler>>,
    task: tokio::task::JoinHandle<()>,
}

impl ActiveShortRecorder {
    pub async fn start() -> Result<Self, String> {
        let device = crate::audio::default_input_device()
            .map_err(|e| format!("Failed to get default input device: {e}"))?;
        let device_name = device.name.clone();
        let running = Arc::new(AtomicBool::new(true));
        let stream = Arc::new(
            AudioStream::from_device(Arc::new(device), running.clone())
                .await
                .map_err(|e| format!("Failed to create voice input audio stream: {e}"))?,
        );
        let native_rate = stream.device_config.sample_rate().0;
        info!(
            "Voice input short recorder stream opened: device={} native_rate={} target_rate={} chunk_size={}",
            device_name,
            native_rate,
            TARGET_SAMPLE_RATE,
            RESAMPLER_CHUNK_SIZE
        );
        let resampler = Arc::new(Mutex::new(
            StreamingResampler::new(native_rate, TARGET_SAMPLE_RATE, RESAMPLER_CHUNK_SIZE)
                .map_err(|e| format!("Failed to create voice input resampler: {e}"))?,
        ));
        let samples = Arc::new(Mutex::new(Vec::new()));
        let mut receiver = stream.subscribe().await;
        let running_for_task = running.clone();
        let samples_for_task = samples.clone();
        let resampler_for_task = resampler.clone();
        let task = tokio::spawn(async move {
            while running_for_task.load(Ordering::SeqCst) {
                match tokio::time::timeout(Duration::from_millis(100), receiver.recv()).await {
                    Ok(Ok(chunk)) => {
                        let processed = {
                            match resampler_for_task.lock() {
                                Ok(mut guard) => {
                                    guard.process_chunk(&chunk).unwrap_or_else(|error| {
                                        warn!("Voice input resampling failed: {error}");
                                        chunk
                                    })
                                }
                                Err(_) => chunk,
                            }
                        };
                        if !processed.is_empty() {
                            if let Ok(mut guard) = samples_for_task.lock() {
                                guard.extend_from_slice(&processed);
                            }
                        }
                    }
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                    Ok(Err(_)) => break,
                    Err(_) => continue,
                }
            }
        });

        Ok(Self {
            running,
            stream,
            samples,
            resampler,
            task,
        })
    }

    pub async fn stop(self) -> Result<Vec<f32>, String> {
        self.running.store(false, Ordering::SeqCst);
        self.stream
            .stop()
            .await
            .map_err(|e| format!("Failed to stop voice input stream: {e}"))?;
        let _ = self.task.await;
        if let Ok(mut resampler) = self.resampler.lock() {
            match resampler.flush() {
                Ok(remaining) => {
                    if !remaining.is_empty() {
                        if let Ok(mut samples) = self.samples.lock() {
                            samples.extend_from_slice(&remaining);
                        }
                    }
                }
                Err(error) => warn!("Failed to flush voice input resampler: {error}"),
            }
        }
        let samples = self
            .samples
            .lock()
            .map_err(|e| format!("Failed to read voice input samples: {e}"))?
            .clone();
        info!(
            "Voice input short recorder collected samples: count={} duration_ms={}",
            samples.len(),
            (samples.len() as u64).saturating_mul(1_000) / TARGET_SAMPLE_RATE as u64
        );
        Ok(samples)
    }
}

pub fn write_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create voice input temp dir: {e}"))?;
    }
    let spec = WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec)
        .map_err(|e| format!("Failed to create voice input WAV: {e}"))?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(value)
            .map_err(|e| format!("Failed to write voice input WAV: {e}"))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize voice input WAV: {e}"))?;
    info!(
        "Voice input WAV written: path={} samples={} duration_ms={}",
        path.display(),
        samples.len(),
        (samples.len() as u64).saturating_mul(1_000) / TARGET_SAMPLE_RATE as u64
    );
    Ok(())
}
