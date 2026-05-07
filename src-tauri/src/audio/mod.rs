// Audio module for simple recorder
pub mod core;
pub mod processing;

pub use core::{
    default_input_device, default_output_device, list_audio_devices, supports_system_audio,
    trigger_audio_permission, trigger_system_audio_permission, AudioStream, DeviceType,
};

pub use processing::{
    mix_streams_aligned, StreamingResampler, BUFFER_FLUSH_INTERVAL_MS, BUFFER_FLUSH_THRESHOLD,
    RESAMPLER_CHUNK_SIZE, TARGET_SAMPLE_RATE,
};
