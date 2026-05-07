use log::{debug, info};
use std::path::Path;

/// Write audio buffer to a temporary file
pub fn write_buffer_to_temp_file(
    buffer: &[f32],
    segment_index: u32,
    channel_type: &str,
    temp_dir: &Path,
) -> Result<(), String> {
    if buffer.is_empty() {
        return Ok(());
    }

    let filename = format!("temp_{}_{:03}.raw", channel_type, segment_index);
    let temp_path = temp_dir.join(filename);

    // Convert f32 samples to bytes
    let bytes: Vec<u8> = buffer
        .iter()
        .flat_map(|&sample| sample.to_le_bytes().to_vec())
        .collect();

    std::fs::write(&temp_path, bytes)
        .map_err(|e| format!("Failed to write temp file {}: {}", temp_path.display(), e))?;

    info!(
        "Wrote {} samples to temp file: {}",
        buffer.len(),
        temp_path.display()
    );
    Ok(())
}

/// Read audio data from a temporary file
pub fn read_temp_file(
    segment_index: u32,
    channel_type: &str,
    temp_dir: &Path,
) -> Result<Vec<f32>, String> {
    let filename = format!("temp_{}_{:03}.raw", channel_type, segment_index);
    let temp_path = temp_dir.join(filename);

    if !temp_path.exists() {
        return Ok(Vec::new());
    }

    let bytes = std::fs::read(&temp_path)
        .map_err(|e| format!("Failed to read temp file {}: {}", temp_path.display(), e))?;

    // Convert bytes back to f32 samples
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| {
            let mut bytes = [0u8; 4];
            bytes.copy_from_slice(chunk);
            f32::from_le_bytes(bytes)
        })
        .collect();

    debug!(
        "Read {} samples from temp file: {}",
        samples.len(),
        temp_path.display()
    );
    Ok(samples)
}

/// Clean up temporary files for a specific segment
pub fn cleanup_temp_files(segment_index: u32, temp_dir: &Path) -> Result<(), String> {
    let mic_file = temp_dir.join(format!("temp_mic_{:03}.raw", segment_index));
    let system_file = temp_dir.join(format!("temp_system_{:03}.raw", segment_index));

    if mic_file.exists() {
        std::fs::remove_file(&mic_file)
            .map_err(|e| format!("Failed to remove temp file {}: {}", mic_file.display(), e))?;
    }

    if system_file.exists() {
        std::fs::remove_file(&system_file).map_err(|e| {
            format!(
                "Failed to remove temp file {}: {}",
                system_file.display(),
                e
            )
        })?;
    }

    debug!("Cleaned up temp files for segment {}", segment_index);
    Ok(())
}
