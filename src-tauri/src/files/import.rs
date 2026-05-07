use crate::storage::get_workspace_recordings_dir;
use crate::types::{ImportedNote, RecordingFile};
use log::{error, info, warn};
use std::path::Path;

/// Convert txt file newlines to Markdown format
/// Converts all line breaks to paragraph separators (double newlines) for unified formatting
fn convert_txt_to_markdown_newlines(text: &str) -> String {
    // Step 1: Normalize line endings to \n
    let mut normalized = text.replace("\r\n", "\n").replace("\r", "\n");

    // Step 2: Handle literal \n sequences (sometimes present in transcription files)
    // Convert literal "\\n" strings to actual newlines
    normalized = normalized.replace("\\n", "\n");

    // Step 3: Remove Markdown hard breaks (backslash at end of line)
    // This ensures clean conversion to paragraph style
    normalized = normalized.replace("\\\n", "\n").replace("\\ \n", "\n");

    // Step 4: Collapse multiple newlines (3+) into double newlines
    let mut processed = String::new();
    let mut prev_was_newline = false;
    let mut consecutive_newlines = 0;

    for ch in normalized.chars() {
        if ch == '\n' {
            consecutive_newlines += 1;
            prev_was_newline = true;
        } else {
            if prev_was_newline {
                // Cap consecutive newlines at 2
                let newlines_to_add = if consecutive_newlines >= 2 { 2 } else { 1 };
                for _ in 0..newlines_to_add {
                    processed.push('\n');
                }
                consecutive_newlines = 0;
                prev_was_newline = false;
            }
            processed.push(ch);
        }
    }
    // Handle trailing newlines
    if prev_was_newline && consecutive_newlines > 0 {
        let newlines_to_add = if consecutive_newlines >= 2 { 2 } else { 1 };
        for _ in 0..newlines_to_add {
            processed.push('\n');
        }
    }

    // Step 5: Convert single newlines to double newlines (paragraph separators)
    // Split by lines, trim each, and rejoin with double newlines
    let lines: Vec<&str> = processed.split('\n').collect();
    let mut result = String::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if !line.is_empty() {
            if !result.is_empty() {
                result.push_str("\n\n");
            }
            result.push_str(line);
        } else if !result.is_empty() && i + 1 < lines.len() {
            // Preserve empty lines as paragraph separators
            // But don't add multiple consecutive paragraph breaks
            if !result.ends_with("\n\n") {
                result.push_str("\n\n");
            }
        }

        i += 1;
    }

    result
}

/// Import audio files to a workspace recordings directory.
pub async fn import_audio_files_to_workspace(
    file_paths: Vec<String>,
    workspace_folder: String,
) -> Result<Vec<RecordingFile>, String> {
    let recordings_dir = get_workspace_recordings_dir(&workspace_folder)?;
    import_audio_files_into_dir(file_paths, recordings_dir).await
}

async fn import_audio_files_into_dir(
    file_paths: Vec<String>,
    recordings_dir: std::path::PathBuf,
) -> Result<Vec<RecordingFile>, String> {
    info!(
        "🎵 [IMPORT] Starting import of {} audio files",
        file_paths.len()
    );
    info!("🎵 [IMPORT] File paths: {:?}", file_paths);
    info!(
        "🎵 [IMPORT] Recordings directory: {}",
        recordings_dir.display()
    );

    let mut imported_recordings = Vec::new();
    let file_paths_len = file_paths.len();

    for (index, source_path) in file_paths.iter().enumerate() {
        info!(
            "🎵 [IMPORT] Processing file {}/{}: {}",
            index + 1,
            file_paths_len,
            source_path
        );
        let source = Path::new(&source_path);

        // Verify source file exists
        if !source.exists() {
            warn!("Source file not found: {}", source_path);
            continue;
        }

        // Get file extension
        let extension = match source.extension().and_then(|ext| ext.to_str()) {
            Some(ext) => ext,
            None => {
                warn!("Invalid file extension: {}", source_path);
                continue;
            }
        };

        // Verify it's a supported audio format
        let ext_lower = extension.to_lowercase();
        info!("🔍 [IMPORT] File extension: {}", ext_lower);
        if !matches!(
            ext_lower.as_str(),
            "flac" | "wav" | "mp3" | "m4a" | "aac" | "ogg"
        ) {
            warn!("⚠️  [IMPORT] Unsupported audio format: {}", extension);
            continue;
        }
        info!("✅ [IMPORT] Supported audio format: {}", ext_lower);

        // Use original filename
        let original_filename = match source.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => {
                warn!("Invalid filename: {}", source_path);
                continue;
            }
        };

        // Check if file already exists
        let new_filename = original_filename.to_string();
        let target_path = recordings_dir.join(&new_filename);

        // If file exists, remove it first (overwrite strategy)
        if target_path.exists() {
            info!(
                "⚠️  [IMPORT] File already exists, will overwrite: {}",
                target_path.display()
            );
            if let Err(e) = std::fs::remove_file(&target_path) {
                warn!("⚠️  [IMPORT] Failed to remove existing file: {}", e);
            }
        }

        info!("📁 [IMPORT] Target path: {}", target_path.display());

        // Copy file to recordings directory
        match std::fs::copy(source, &target_path) {
            Ok(bytes_copied) => {
                info!(
                    "✅ [IMPORT] Successfully copied {} bytes: {} -> {}",
                    bytes_copied,
                    source_path,
                    target_path.display()
                );

                // Get file metadata
                if let Ok(metadata) = std::fs::metadata(&target_path) {
                    let created_time = metadata
                        .created()
                        .or_else(|_| metadata.modified())
                        .map(|time| {
                            let datetime: chrono::DateTime<chrono::Local> = time.into();
                            datetime.format("%Y-%m-%d %H:%M:%S").to_string()
                        })
                        .unwrap_or_else(|_| "Unknown".to_string());

                    let size = metadata.len();
                    let id = crate::db::recording_id_from_path(&target_path);

                    let recording_file = RecordingFile {
                        id: id.clone(),
                        name: new_filename.clone(),
                        path: target_path.to_string_lossy().to_string(),
                        created: created_time.clone(),
                        size,
                    };

                    info!(
                        "📝 [IMPORT] Created RecordingFile: id={}, name={}, path={}, size={}",
                        recording_file.id,
                        recording_file.name,
                        recording_file.path,
                        recording_file.size
                    );

                    imported_recordings.push(recording_file);
                }
            }
            Err(e) => {
                error!("❌ [IMPORT] Failed to copy {}: {}", source_path, e);
                continue;
            }
        }
    }

    info!(
        "🎉 [IMPORT] Successfully imported {} out of {} files",
        imported_recordings.len(),
        file_paths_len
    );
    info!(
        "📋 [IMPORT] Imported files: {:?}",
        imported_recordings
            .iter()
            .map(|f| &f.name)
            .collect::<Vec<_>>()
    );
    Ok(imported_recordings)
}

/// Import note files (markdown, text, docx)
pub async fn import_note_files(file_paths: Vec<String>) -> Result<Vec<ImportedNote>, String> {
    info!("Importing {} note files...", file_paths.len());

    let mut imported_notes = Vec::new();
    let file_paths_len = file_paths.len();

    for source_path in &file_paths {
        let source = Path::new(&source_path);

        // Verify source file exists
        if !source.exists() {
            warn!("Source file not found: {}", source_path);
            continue;
        }

        // Get file extension and name
        let extension = source
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_lowercase());

        let file_name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled");

        let title = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();

        // Read and parse file content based on format
        let content = match extension.as_deref() {
            Some("md") => {
                // Read markdown files directly
                match std::fs::read_to_string(source) {
                    Ok(text) => text,
                    Err(e) => {
                        error!("Failed to read file {}: {}", source_path, e);
                        continue;
                    }
                }
            }
            Some("txt") => {
                // Read text files and convert newlines to Markdown format
                match std::fs::read_to_string(source) {
                    Ok(text) => {
                        // Convert txt newlines to Markdown newlines
                        // Strategy: Add two spaces before each newline for proper line breaks in Markdown
                        convert_txt_to_markdown_newlines(&text)
                    }
                    Err(e) => {
                        error!("Failed to read file {}: {}", source_path, e);
                        continue;
                    }
                }
            }
            Some("docx") => {
                // Read DOCX file bytes first
                let bytes = match std::fs::read(source) {
                    Ok(b) => b,
                    Err(e) => {
                        error!("Failed to read DOCX file {}: {}", source_path, e);
                        continue;
                    }
                };

                // Parse DOCX file and extract text
                match docx_rs::read_docx(&bytes) {
                    Ok(docx) => {
                        // Extract text from all paragraphs
                        let mut text_parts = Vec::new();

                        // Extract text from document body
                        for child in &docx.document.children {
                            if let docx_rs::DocumentChild::Paragraph(para) = child {
                                let mut para_text = String::new();
                                for para_child in &para.children {
                                    if let docx_rs::ParagraphChild::Run(run) = para_child {
                                        for run_child in &run.children {
                                            if let docx_rs::RunChild::Text(text) = run_child {
                                                para_text.push_str(&text.text);
                                            }
                                        }
                                    }
                                }
                                if !para_text.is_empty() {
                                    text_parts.push(para_text);
                                }
                            }
                        }

                        text_parts.join("\n\n")
                    }
                    Err(e) => {
                        error!("Failed to parse DOCX file {}: {}", source_path, e);
                        continue;
                    }
                }
            }
            _ => {
                warn!("Unsupported file format: {:?}", extension);
                continue;
            }
        };

        info!("Successfully imported note: {}", file_name);
        imported_notes.push(ImportedNote {
            title,
            content,
            file_name: file_name.to_string(),
        });
    }

    info!(
        "Successfully imported {} out of {} note files",
        imported_notes.len(),
        file_paths_len
    );
    Ok(imported_notes)
}
