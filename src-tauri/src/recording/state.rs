use crate::audio::{AudioStream, StreamingResampler};
use lazy_static::lazy_static;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

// Global recording state
lazy_static! {
    pub static ref RECORDING_STATE: Arc<Mutex<RecordingState>> =
        Arc::new(Mutex::new(RecordingState::default()));
}

pub static IS_RECORDING: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
pub struct RecordingState {
    pub start_time: Option<Instant>,
    pub target_sample_rate: u32,
    pub stream: Option<Arc<AudioStream>>,
    pub system_stream: Option<Arc<AudioStream>>,
    pub recording_mode: RecordingMode,
    // Native sample rate tracking for proper processing
    pub mic_native_rate: u32,
    pub system_native_rate: u32,
    // Dual buffer architecture with timestamp synchronization
    pub mic_buffer: Option<Arc<Mutex<Vec<f32>>>>,
    pub system_buffer: Option<Arc<Mutex<Vec<f32>>>>,
    // Streaming resamplers for real-time processing
    pub mic_resampler: Option<Arc<Mutex<StreamingResampler>>>,
    pub system_resampler: Option<Arc<Mutex<StreamingResampler>>>,
    // Streaming file processing
    pub segment_index: u32,
    pub temp_dir: Option<PathBuf>,
    pub last_flush_time: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
pub enum RecordingMode {
    #[default]
    MicrophoneOnly,
    MixedAudio,
}

impl RecordingState {
    pub fn reset(&mut self) {
        self.start_time = None;
        self.stream = None;
        self.system_stream = None;
        self.mic_buffer = None;
        self.system_buffer = None;
        self.mic_resampler = None;
        self.system_resampler = None;
        self.mic_native_rate = 0;
        self.system_native_rate = 0;
        self.segment_index = 0;
        self.temp_dir = None;
        self.last_flush_time = None;
    }
}

// Helper functions for recording state
pub fn is_recording() -> bool {
    IS_RECORDING.load(Ordering::SeqCst)
}

pub fn set_recording(value: bool) {
    IS_RECORDING.store(value, Ordering::SeqCst);
}
