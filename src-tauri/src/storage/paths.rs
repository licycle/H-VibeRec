use std::path::PathBuf;

/// Get the application data directory
pub fn get_app_data_dir() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("VOICE_VIBE_TEST_APP_DATA_DIR") {
        let app_data = PathBuf::from(path);
        std::fs::create_dir_all(&app_data)
            .map_err(|e| format!("Failed to create app data directory: {}", e))?;
        return Ok(app_data);
    }

    // Use user documents directory with app-specific folder
    if let Some(mut app_data) = dirs::document_dir() {
        app_data.push("Voice Vibe Local");
        std::fs::create_dir_all(&app_data)
            .map_err(|e| format!("Failed to create app data directory: {}", e))?;
        Ok(app_data)
    } else {
        // Fallback to home directory
        if let Some(mut home) = dirs::home_dir() {
            home.push(".voice-vibe-local");
            std::fs::create_dir_all(&home)
                .map_err(|e| format!("Failed to create app data directory: {}", e))?;
            Ok(home)
        } else {
            Err("Failed to get app data directory".to_string())
        }
    }
}

/// Get the local workspaces root directory.
pub fn get_workspaces_dir() -> Result<PathBuf, String> {
    let workspaces_dir = get_app_data_dir()?.join("workspaces");
    std::fs::create_dir_all(&workspaces_dir)
        .map_err(|e| format!("Failed to create workspaces directory: {}", e))?;
    Ok(workspaces_dir)
}

/// Build a local workspace path without creating it.
pub fn workspace_dir_path(workspace_folder: &str) -> Result<PathBuf, String> {
    let folder = sanitize_workspace_folder(workspace_folder);
    Ok(get_workspaces_dir()?.join(folder))
}

/// Build a local workspace path only when the requested folder is already canonical.
pub fn strict_workspace_dir_path(workspace_folder: &str) -> Result<PathBuf, String> {
    let raw = workspace_folder.trim();
    if raw.is_empty() {
        return Err("workspaceFolder is required".to_string());
    }

    let sanitized = sanitize_workspace_folder(raw);
    if sanitized != raw {
        return Err("workspaceFolder contains invalid characters".to_string());
    }

    let workspaces_dir = get_workspaces_dir()?;
    let candidate = workspaces_dir.join(&sanitized);
    if candidate.parent() != Some(workspaces_dir.as_path()) {
        return Err("workspaceFolder must stay inside app workspaces".to_string());
    }
    Ok(candidate)
}

/// Get a local workspace directory under the app data directory.
pub fn get_workspace_dir(workspace_folder: &str) -> Result<PathBuf, String> {
    let workspace_dir = workspace_dir_path(workspace_folder)?;
    std::fs::create_dir_all(&workspace_dir)
        .map_err(|e| format!("Failed to create workspace directory: {}", e))?;
    Ok(workspace_dir)
}

/// Get the recordings directory for a local workspace.
pub fn get_workspace_recordings_dir(workspace_folder: &str) -> Result<PathBuf, String> {
    let recordings_dir = get_workspace_dir(workspace_folder)?.join("recordings");
    std::fs::create_dir_all(&recordings_dir)
        .map_err(|e| format!("Failed to create workspace recordings directory: {}", e))?;
    Ok(recordings_dir)
}

fn sanitize_workspace_folder(value: &str) -> String {
    let cleaned = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if cleaned.is_empty() {
        "local-workspace".to_string()
    } else {
        cleaned
    }
}

/// Get the temporary files directory
pub fn get_temp_dir() -> Result<PathBuf, String> {
    let app_data = get_app_data_dir()?;
    let temp_dir = app_data.join("temp");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;
    Ok(temp_dir)
}

/// Validate an explicit source path.
pub fn find_source_file(source_path: &str) -> Result<String, String> {
    if std::path::Path::new(source_path).exists() {
        return Ok(source_path.to_string());
    }
    Err(format!("Source file not found at: {}", source_path))
}
