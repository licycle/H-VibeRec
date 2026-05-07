pub mod encoding;
pub mod paths;
pub mod temp_files;

// Re-export commonly used functions
pub use encoding::stream_merge_temp_files_to_wav;
pub use paths::{
    find_source_file, get_app_data_dir, get_temp_dir, get_workspace_dir,
    get_workspace_recordings_dir, get_workspaces_dir, strict_workspace_dir_path,
    workspace_dir_path,
};
pub use temp_files::write_buffer_to_temp_file;
