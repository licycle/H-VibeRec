use crate::types::{ImportedNote, LocalNote, RecordingFile};
use log::info;

#[tauri::command]
#[allow(non_snake_case)]
pub async fn list_workspace_recordings(
    workspaceFolder: String,
) -> Result<Vec<RecordingFile>, String> {
    crate::files::list_workspace_recordings(workspaceFolder).await
}

#[tauri::command]
pub async fn list_workspace_dirs() -> Result<Vec<String>, String> {
    crate::files::list_workspace_dirs().await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn ensure_workspace_dir(workspaceFolder: String) -> Result<String, String> {
    crate::files::ensure_workspace_dir(workspaceFolder).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn list_workspace_notes(workspaceFolder: String) -> Result<Vec<LocalNote>, String> {
    crate::files::list_workspace_notes(workspaceFolder).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_workspace_note(
    workspaceFolder: String,
    note: LocalNote,
) -> Result<LocalNote, String> {
    crate::files::save_workspace_note(workspaceFolder, note).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn delete_workspace_note(workspaceFolder: String, noteId: String) -> Result<(), String> {
    crate::files::delete_workspace_note(workspaceFolder, noteId).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn delete_workspace_recording(
    workspaceFolder: String,
    recordingId: String,
) -> Result<(), String> {
    info!(
        "[delete_workspace_recording command] workspaceFolder={}, recordingId={}",
        workspaceFolder, recordingId
    );
    crate::files::delete_workspace_recording(workspaceFolder, recordingId).await
}

#[tauri::command]
pub async fn delete_audio_file(file_path: String) -> Result<(), String> {
    info!("[delete_audio_file command] file_path={}", file_path);
    crate::files::delete_audio_file(file_path).await
}

#[tauri::command]
pub async fn play_audio_file(file_path: String) -> Result<(), String> {
    crate::files::play_audio_file(file_path).await
}

#[tauri::command]
pub async fn export_audio_file(source_path: String, target_path: String) -> Result<(), String> {
    crate::files::export_audio_file(source_path, target_path).await
}

#[tauri::command]
pub async fn save_text_file(content: String, target_path: String) -> Result<(), String> {
    crate::files::save_text_file(content, target_path).await
}

#[tauri::command]
pub async fn import_audio_files_to_workspace(
    file_paths: Vec<String>,
    #[allow(non_snake_case)] workspaceFolder: String,
) -> Result<Vec<RecordingFile>, String> {
    crate::files::import_audio_files_to_workspace(file_paths, workspaceFolder).await
}

#[tauri::command]
pub async fn import_note_files(file_paths: Vec<String>) -> Result<Vec<ImportedNote>, String> {
    crate::files::import_note_files(file_paths).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_workspace_recording_save_path(workspaceFolder: String) -> Result<String, String> {
    crate::files::get_workspace_recording_save_path(workspaceFolder).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn delete_workspace_dir(workspaceFolder: String) -> Result<(), String> {
    crate::files::delete_workspace_dir(workspaceFolder).await
}
