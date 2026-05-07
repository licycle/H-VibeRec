use crate::storage::{find_source_file, get_workspace_recordings_dir};
use crate::types::{LocalNote, RecordingFile};
use log::info;
use std::path::{Path, PathBuf};

/// List recordings in a single local workspace folder.
pub async fn list_workspace_recordings(
    workspace_folder: String,
) -> Result<Vec<RecordingFile>, String> {
    let recordings_dir = get_workspace_recordings_dir(&workspace_folder)?;
    info!(
        "📋 [LIST_WORKSPACE] Listing recordings for workspace {} in {}",
        workspace_folder,
        recordings_dir.display()
    );

    let mut recordings = scan_recordings_dir(&recordings_dir)?;
    recordings.sort_by(|a, b| b.created.cmp(&a.created));
    info!(
        "🎉 [LIST_WORKSPACE] Found {} recordings for workspace {}",
        recordings.len(),
        workspace_folder
    );
    Ok(recordings)
}

/// List local workspace folders from disk.
pub async fn list_workspace_dirs() -> Result<Vec<String>, String> {
    let workspaces_dir = crate::storage::get_workspaces_dir()?;
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(&workspaces_dir)
        .map_err(|e| format!("Failed to read workspaces directory: {}", e))?
        .flatten()
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            folders.push(name.to_string());
        }
    }
    folders.sort();
    Ok(folders)
}

/// Ensure a workspace directory exists.
pub async fn ensure_workspace_dir(workspace_folder: String) -> Result<String, String> {
    let workspace_dir = crate::storage::get_workspace_dir(&workspace_folder)?;
    Ok(workspace_dir.to_string_lossy().to_string())
}

fn get_workspace_notes_dir(workspace_folder: &str) -> Result<PathBuf, String> {
    let notes_dir = crate::storage::get_workspace_dir(workspace_folder)?.join("notes");
    std::fs::create_dir_all(&notes_dir)
        .map_err(|e| format!("Failed to create workspace notes directory: {}", e))?;
    Ok(notes_dir)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn file_time_iso(metadata: &std::fs::Metadata) -> String {
    metadata
        .modified()
        .or_else(|_| metadata.created())
        .map(|time| {
            let datetime: chrono::DateTime<chrono::Utc> = time.into();
            datetime.to_rfc3339()
        })
        .unwrap_or_else(|_| now_iso())
}

fn sanitize_note_title(title: &str) -> String {
    let cleaned = title
        .trim()
        .chars()
        .map(|ch| {
            if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || ch.is_control()
            {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();

    let cleaned = if cleaned.is_empty() {
        "未命名笔记".to_string()
    } else {
        cleaned
    };

    cleaned.chars().take(80).collect()
}

fn note_file_name(note: &LocalNote) -> String {
    format!("{}__{}.md", note.id, sanitize_note_title(&note.title))
}

fn split_note_file_stem(path: &Path) -> Option<(String, String)> {
    let stem = path.file_stem()?.to_str()?;
    let (id, title) = stem.split_once("__")?;
    if id.trim().is_empty() {
        return None;
    }
    Some((id.to_string(), title.to_string()))
}

fn find_note_paths(notes_dir: &Path, note_id: &str) -> Result<Vec<PathBuf>, String> {
    if !notes_dir.exists() {
        return Ok(Vec::new());
    }

    let prefix = format!("{}__", note_id);
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(notes_dir)
        .map_err(|e| format!("Failed to read notes directory: {}", e))?
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        if path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(|stem| stem.starts_with(&prefix))
            .unwrap_or(false)
        {
            matches.push(path);
        }
    }

    Ok(matches)
}

/// List notes stored in a single local workspace folder.
pub async fn list_workspace_notes(workspace_folder: String) -> Result<Vec<LocalNote>, String> {
    let notes_dir = get_workspace_notes_dir(&workspace_folder)?;
    let mut notes = Vec::new();

    for entry in std::fs::read_dir(&notes_dir)
        .map_err(|e| format!("Failed to read notes directory: {}", e))?
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        let Some((id, title)) = split_note_file_stem(&path) else {
            continue;
        };

        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read note file {}: {}", path.display(), e))?;
        let metadata = std::fs::metadata(&path)
            .map_err(|e| format!("Failed to read note metadata {}: {}", path.display(), e))?;
        let created = metadata
            .created()
            .map(|time| {
                let datetime: chrono::DateTime<chrono::Utc> = time.into();
                datetime.to_rfc3339()
            })
            .unwrap_or_else(|_| file_time_iso(&metadata));
        let updated = file_time_iso(&metadata);

        notes.push(LocalNote {
            id,
            title,
            content,
            created,
            updated,
        });
    }

    notes.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(notes)
}

/// Save a note into a local workspace notes folder.
pub async fn save_workspace_note(
    workspace_folder: String,
    note: LocalNote,
) -> Result<LocalNote, String> {
    let notes_dir = get_workspace_notes_dir(&workspace_folder)?;
    let note = LocalNote {
        title: note.title.trim().to_string(),
        updated: if note.updated.trim().is_empty() {
            now_iso()
        } else {
            note.updated
        },
        created: if note.created.trim().is_empty() {
            now_iso()
        } else {
            note.created
        },
        ..note
    };
    let note = LocalNote {
        title: if note.title.is_empty() {
            "未命名笔记".to_string()
        } else {
            note.title
        },
        ..note
    };
    let target_path = notes_dir.join(note_file_name(&note));

    for existing_path in find_note_paths(&notes_dir, &note.id)? {
        if existing_path != target_path {
            std::fs::remove_file(&existing_path).map_err(|e| {
                format!(
                    "Failed to remove old note file {}: {}",
                    existing_path.display(),
                    e
                )
            })?;
        }
    }

    std::fs::write(&target_path, &note.content)
        .map_err(|e| format!("Failed to save note file {}: {}", target_path.display(), e))?;
    Ok(note)
}

/// Delete a note from a local workspace notes folder.
pub async fn delete_workspace_note(
    workspace_folder: String,
    note_id: String,
) -> Result<(), String> {
    let notes_dir = get_workspace_notes_dir(&workspace_folder)?;
    for path in find_note_paths(&notes_dir, &note_id)? {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to delete note file {}: {}", path.display(), e))?;
    }
    Ok(())
}

/// Delete a recording from a local workspace by its stable recording id.
pub async fn delete_workspace_recording(
    workspace_folder: String,
    recording_id: String,
) -> Result<(), String> {
    let recordings_dir = get_workspace_recordings_dir(&workspace_folder)?;
    info!(
        "[delete_workspace_recording] request workspace={}, recording_id={}, recordings_dir={}",
        workspace_folder,
        recording_id,
        recordings_dir.display()
    );

    let recordings = scan_recordings_dir(&recordings_dir)?;
    info!(
        "[delete_workspace_recording] scanned {} recording files",
        recordings.len()
    );
    for recording in &recordings {
        info!(
            "[delete_workspace_recording] candidate id={}, name={}, path={}",
            recording.id, recording.name, recording.path
        );
    }

    let mut deleted_file = false;
    let mut matched_recording_ids = Vec::new();

    for recording in recordings {
        if !recording_matches_delete_request(&recording, &recording_id, &workspace_folder) {
            continue;
        }

        info!(
            "[delete_workspace_recording] matched file id={}, requested_id={}, path={}",
            recording.id, recording_id, recording.path
        );
        push_unique(&mut matched_recording_ids, recording.id.clone());
        let path = PathBuf::from(&recording.path);
        match std::fs::remove_file(&path) {
            Ok(()) => {
                deleted_file = true;
                info!(
                    "[delete_workspace_recording] deleted workspace recording file: {}",
                    path.display()
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                deleted_file = true;
                info!(
                    "[delete_workspace_recording] workspace recording file already missing: {}",
                    path.display()
                );
            }
            Err(error) => {
                info!(
                    "[delete_workspace_recording] failed deleting file {}: {}",
                    path.display(),
                    error
                );
                return Err(format!(
                    "Failed to delete workspace recording file {}: {}",
                    path.display(),
                    error
                ));
            }
        }
    }

    let metadata_ids =
        metadata_delete_candidates(&workspace_folder, &recording_id, &matched_recording_ids);
    let mut deleted_metadata = false;
    for id in &metadata_ids {
        match crate::db::delete_recording(id) {
            Ok(()) => {
                deleted_metadata = true;
                info!(
                    "[delete_workspace_recording] deleted local recording metadata: {}",
                    id
                );
            }
            Err(error) => {
                info!(
                    "[delete_workspace_recording] metadata not deleted for {}: {}",
                    id, error
                );
            }
        }
    }

    let deleted_normalized =
        delete_normalized_audio_candidates(&workspace_folder, &recording_id, &metadata_ids)?;

    info!(
        "[delete_workspace_recording] result workspace={}, recording_id={}, deleted_file={}, deleted_metadata={}, deleted_normalized={}",
        workspace_folder,
        recording_id,
        deleted_file,
        deleted_metadata,
        deleted_normalized
    );

    if deleted_file || deleted_metadata || deleted_normalized {
        return Ok(());
    }

    Err(format!(
        "Recording not found in workspace {}: {}",
        workspace_folder, recording_id
    ))
}

fn recording_matches_delete_request(
    recording: &RecordingFile,
    requested_id: &str,
    workspace_folder: &str,
) -> bool {
    if recording.id == requested_id {
        return true;
    }

    let requested_stem = recording_id_stem(requested_id, workspace_folder);
    let prefixed_requested_id = format!("{workspace_folder}__{requested_stem}");
    if recording.id == prefixed_requested_id {
        return true;
    }

    let file_stem = Path::new(&recording.path)
        .file_stem()
        .and_then(|value| value.to_str());

    file_stem == Some(requested_id) || file_stem == Some(requested_stem.as_str())
}

fn recording_id_stem(recording_id: &str, workspace_folder: &str) -> String {
    let workspace_prefix = format!("{workspace_folder}__");
    if let Some(stem) = recording_id.strip_prefix(&workspace_prefix) {
        return stem.to_string();
    }

    if let Some((_, stem)) = recording_id.split_once("__") {
        return stem.to_string();
    }

    recording_id.to_string()
}

fn metadata_delete_candidates(
    workspace_folder: &str,
    recording_id: &str,
    matched_recording_ids: &[String],
) -> Vec<String> {
    let mut candidates = Vec::new();
    let stem = recording_id_stem(recording_id, workspace_folder);

    push_unique(&mut candidates, recording_id.to_string());
    push_unique(&mut candidates, format!("{workspace_folder}__{stem}"));
    push_unique(&mut candidates, stem);
    for id in matched_recording_ids {
        push_unique(&mut candidates, id.clone());
    }

    candidates
}

fn delete_normalized_audio_candidates(
    workspace_folder: &str,
    recording_id: &str,
    metadata_ids: &[String],
) -> Result<bool, String> {
    let stem = recording_id_stem(recording_id, workspace_folder);
    let mut candidate_ids = Vec::new();

    for id in metadata_ids {
        push_unique(&mut candidate_ids, id.clone());
    }
    push_unique(&mut candidate_ids, recording_id.to_string());
    push_unique(&mut candidate_ids, format!("{workspace_folder}__{stem}"));
    push_unique(&mut candidate_ids, stem);

    delete_normalized_audio_ids(&candidate_ids)
}

fn delete_normalized_audio_ids(candidate_ids: &[String]) -> Result<bool, String> {
    let normalized_dir = crate::db::normalized_audio_dir()?;
    let mut deleted = false;
    for id in candidate_ids {
        let path = normalized_dir.join(format!("{id}.wav"));
        if !path.exists() {
            continue;
        }

        std::fs::remove_file(&path).map_err(|error| {
            format!(
                "Failed to delete normalized audio {}: {}",
                path.display(),
                error
            )
        })?;
        deleted = true;
        info!(
            "[delete_workspace_recording] deleted normalized audio file: {}",
            path.display()
        );
    }

    Ok(deleted)
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.trim().is_empty() && !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn scan_recordings_dir(recordings_dir: &Path) -> Result<Vec<RecordingFile>, String> {
    let mut recordings = Vec::new();

    if !recordings_dir.exists() {
        return Ok(recordings);
    }

    let entries = std::fs::read_dir(recordings_dir)
        .map_err(|e| format!("Failed to read recordings directory: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let is_audio = path.is_file()
            && path.extension().map_or(false, |ext| {
                let ext_str = ext.to_str().unwrap_or("").to_lowercase();
                matches!(
                    ext_str.as_str(),
                    "flac" | "wav" | "mp3" | "m4a" | "aac" | "ogg"
                )
            });

        if !is_audio {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");

            let created_time = metadata
                .created()
                .or_else(|_| metadata.modified())
                .map(|time| {
                    let datetime: chrono::DateTime<chrono::Local> = time.into();
                    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or_else(|_| "Unknown".to_string());

            let recording = RecordingFile {
                id: crate::db::recording_id_from_path(&path),
                name: file_name.to_string(),
                path: path.to_string_lossy().to_string(),
                created: created_time,
                size: metadata.len(),
            };

            info!(
                "📝 [LIST_WORKSPACE] Found recording: id={}, name={}, path={}, size={}",
                recording.id, recording.name, recording.path, recording.size
            );
            recordings.push(recording);
        }
    }

    Ok(recordings)
}

/// Delete an audio file
pub async fn delete_audio_file(file_path: String) -> Result<(), String> {
    info!("[delete_audio_file] request path={}", file_path);

    let requested_path = Path::new(&file_path);
    let actual_file_path = match find_source_file(&file_path) {
        Ok(path) => PathBuf::from(path),
        Err(error) => {
            let recording_id = crate::db::recording_id_from_path(requested_path);
            info!(
                "[delete_audio_file] source path missing, trying metadata cleanup id={}, path={}, error={}",
                recording_id,
                file_path,
                error
            );
            match crate::db::delete_recording(&recording_id) {
                Ok(()) => {
                    info!(
                        "[delete_audio_file] deleted stale local recording metadata for missing file: {}",
                        recording_id
                    );
                }
                Err(db_error) => {
                    info!(
                        "[delete_audio_file] audio file already missing and no metadata was deleted: path={}, file_error={}, metadata_error={}",
                        file_path,
                        error,
                        db_error
                    );
                }
            }
            let metadata_ids = vec![recording_id];
            let _ = delete_normalized_audio_ids(&metadata_ids)?;
            return Ok(());
        }
    };

    info!(
        "[delete_audio_file] resolved path={}",
        actual_file_path.display()
    );

    let recording_id = crate::db::recording_id_from_path(&actual_file_path);
    info!(
        "[delete_audio_file] derived recording_id={} from path={}",
        recording_id,
        actual_file_path.display()
    );
    match crate::db::delete_recording(&recording_id) {
        Ok(()) => {
            info!(
                "[delete_audio_file] recording file and local metadata deleted successfully: {}",
                recording_id
            );
            return Ok(());
        }
        Err(db_error) => {
            info!(
                "[delete_audio_file] no local recording metadata deleted for {}: {}. Falling back to file removal.",
                recording_id, db_error
            );
        }
    }

    std::fs::remove_file(&actual_file_path).map_err(|e| format!("Failed to delete file: {}", e))?;
    let metadata_ids = vec![recording_id];
    let _ = delete_normalized_audio_ids(&metadata_ids)?;

    info!(
        "[delete_audio_file] file deleted successfully: {}",
        actual_file_path.display()
    );
    Ok(())
}

/// Play an audio file using the system's default player
pub async fn play_audio_file(file_path: String) -> Result<(), String> {
    info!("Playing audio file: {}", file_path);

    if !std::path::Path::new(&file_path).exists() {
        return Err("Audio file not found".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&file_path)
            .spawn()
            .map_err(|e| format!("Failed to open audio file: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(&["/C", "start", "", &file_path])
            .spawn()
            .map_err(|e| format!("Failed to open audio file: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&file_path)
            .spawn()
            .map_err(|e| format!("Failed to open audio file: {}", e))?;
    }

    Ok(())
}

/// Get the save path for a new recording in a local workspace folder.
pub async fn get_workspace_recording_save_path(workspace_folder: String) -> Result<String, String> {
    let recordings_dir = get_workspace_recordings_dir(&workspace_folder)?;
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let filename = format!("recording-{}.wav", timestamp);
    let save_path = recordings_dir.join(filename);
    Ok(save_path.to_string_lossy().to_string())
}

/// Delete a local workspace directory and all files inside it.
pub async fn delete_workspace_dir(workspace_folder: String) -> Result<(), String> {
    let workspace_dir = crate::storage::workspace_dir_path(&workspace_folder)?;
    if workspace_dir.exists() {
        std::fs::remove_dir_all(&workspace_dir)
            .map_err(|e| format!("Failed to delete workspace directory: {}", e))?;
    }
    Ok(())
}
