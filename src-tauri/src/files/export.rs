use crate::storage::find_source_file;
use log::info;

/// Export an audio file to a target location
pub async fn export_audio_file(source_path: String, target_path: String) -> Result<(), String> {
    info!(
        "Exporting audio file from {} to {}",
        source_path, target_path
    );

    // Debug: check current working directory
    if let Ok(cwd) = std::env::current_dir() {
        info!("Current working directory: {}", cwd.display());
    }

    // Find the actual source file path
    let actual_source_path = find_source_file(&source_path)?;
    info!("Using source file: {}", actual_source_path);

    if let Some(parent) = std::path::Path::new(&target_path).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create export directory: {}", e))?;
        }
    }

    std::fs::copy(&actual_source_path, &target_path)
        .map_err(|e| format!("Failed to export audio file: {}", e))?;

    info!("Audio file exported successfully");
    Ok(())
}

/// Save a text file to a target location
pub async fn save_text_file(content: String, target_path: String) -> Result<(), String> {
    info!("Saving text file to {}", target_path);

    // Ensure target directory exists
    if let Some(parent) = std::path::Path::new(&target_path).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }
    }

    // Write file content
    std::fs::write(&target_path, content.as_bytes())
        .map_err(|e| format!("Failed to write text file: {}", e))?;

    info!("Text file saved successfully to {}", target_path);
    Ok(())
}
