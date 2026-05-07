use crate::audio::{mix_streams_aligned, TARGET_SAMPLE_RATE};
use crate::recording::state::RecordingMode;
use crate::storage::temp_files::{cleanup_temp_files, read_temp_file};
use hound::{SampleFormat, WavSpec, WavWriter};
use log::{info, warn};
use std::path::Path;

/// Merge temporary audio segments and encode to WAV
pub async fn stream_merge_temp_files_to_wav(
    segment_count: u32,
    temp_dir: &Path,
    output_path: &str,
    recording_mode: &RecordingMode,
) -> Result<(), String> {
    info!(
        "Starting stream merge of {} segments to WAV: {}",
        segment_count, output_path
    );

    let mut all_samples = Vec::new();

    // Process each segment
    for segment_idx in 0..segment_count {
        info!("Processing segment {}/{}", segment_idx + 1, segment_count);

        // Read segment data
        let mic_segment = read_temp_file(segment_idx, "mic", temp_dir)?;

        let final_segment_data = match recording_mode {
            RecordingMode::MixedAudio => {
                let system_segment = read_temp_file(segment_idx, "system", temp_dir)?;
                if !mic_segment.is_empty() || !system_segment.is_empty() {
                    // Mix the segment data
                    mix_audio_final(&mic_segment, &system_segment, 0.5, 0.5)
                } else {
                    Vec::new()
                }
            }
            RecordingMode::MicrophoneOnly => mic_segment,
        };

        // Append to all samples
        all_samples.extend_from_slice(&final_segment_data);

        // Clean up temp files for this segment
        if let Err(e) = cleanup_temp_files(segment_idx, temp_dir) {
            warn!(
                "Failed to cleanup temp files for segment {}: {}",
                segment_idx, e
            );
        }
    }

    info!("Merged all segments: {} total samples", all_samples.len());

    if all_samples.is_empty() {
        return Err("No audio data to encode".to_string());
    }

    // Create WAV file with proper specifications
    info!("Creating WAV file: {}", output_path);
    let spec = WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut writer = WavWriter::create(output_path, spec)
        .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

    // Write samples to WAV file
    info!("Writing {} f32 samples to WAV file", all_samples.len());
    for &sample in all_samples.iter() {
        let clamped = sample.max(-1.0).min(1.0);
        let sample_i16 = (clamped * 32767.0) as i16;
        writer
            .write_sample(sample_i16)
            .map_err(|e| format!("Failed to write WAV sample: {}", e))?;
    }

    // Finalize WAV file
    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize WAV file: {}", e))?;

    info!(
        "Successfully merged and encoded {} segments to WAV: {}",
        segment_count, output_path
    );
    Ok(())
}

/// Legacy mixing function - delegates to the optimized version
fn mix_audio_final(
    mic_samples: &[f32],
    system_samples: &[f32],
    mic_gain: f32,
    system_gain: f32,
) -> Vec<f32> {
    mix_streams_aligned(mic_samples, system_samples, mic_gain, system_gain)
}
