use crate::audio::{BUFFER_FLUSH_INTERVAL_MS, BUFFER_FLUSH_THRESHOLD};
use crate::recording::state::RECORDING_STATE;
use crate::storage::temp_files::write_buffer_to_temp_file;
use log::info;
use std::path::PathBuf;
use std::time::Instant;

pub fn should_flush_buffer(buffer_len: usize, last_flush_time: Option<Instant>) -> bool {
    // Flush if buffer exceeds threshold
    if buffer_len >= BUFFER_FLUSH_THRESHOLD {
        return true;
    }

    // Flush if time interval exceeded
    if let Some(last_flush) = last_flush_time {
        if last_flush.elapsed().as_millis() >= BUFFER_FLUSH_INTERVAL_MS as u128 {
            return true;
        }
    }

    false
}

pub async fn flush_buffers_to_temp_files(
    temp_dir: PathBuf,
    segment_index: u32,
) -> Result<(), String> {
    let state = RECORDING_STATE
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    // Extract buffer data and clear buffers
    let mic_data = if let Some(buffer) = &state.mic_buffer {
        if let Ok(mut guard) = buffer.lock() {
            let data = guard.clone();
            guard.clear();
            data
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let system_data = if let Some(buffer) = &state.system_buffer {
        if let Ok(mut guard) = buffer.lock() {
            let data = guard.clone();
            guard.clear();
            data
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    drop(state); // Release lock before file I/O

    // Write buffers to temp files
    if !mic_data.is_empty() {
        write_buffer_to_temp_file(&mic_data, segment_index, "mic", &temp_dir)?;
    }

    if !system_data.is_empty() {
        write_buffer_to_temp_file(&system_data, segment_index, "system", &temp_dir)?;
    }

    info!(
        "Flushed segment {} to temp files: {} mic samples, {} system samples",
        segment_index,
        mic_data.len(),
        system_data.len()
    );

    Ok(())
}

pub async fn flush_mic_buffer_to_temp_file(
    temp_dir: PathBuf,
    segment_index: u32,
) -> Result<(), String> {
    let state = RECORDING_STATE
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    // Extract mic buffer data and clear buffer
    let mic_data = if let Some(buffer) = &state.mic_buffer {
        if let Ok(mut guard) = buffer.lock() {
            let data = guard.clone();
            guard.clear();
            data
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    drop(state); // Release lock before file I/O

    // Write mic buffer to temp file
    if !mic_data.is_empty() {
        write_buffer_to_temp_file(&mic_data, segment_index, "mic", &temp_dir)?;
    }

    info!(
        "Flushed mic segment {} to temp file: {} samples",
        segment_index,
        mic_data.len()
    );

    Ok(())
}
