use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PasteMode {
    #[default]
    Manual,
    Automatic,
    Notify,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasteTarget {
    pub id: String,
    pub app_name: String,
    pub bundle_id: String,
    pub window_title: String,
    #[serde(default)]
    pub process_id: Option<i64>,
    #[serde(default)]
    pub window_id: Option<u32>,
    #[serde(default)]
    pub identity_method: Option<String>,
    #[serde(default)]
    pub availability: Option<String>,
    #[serde(default)]
    pub unavailable_reason: Option<String>,
    #[serde(default)]
    pub control_role: Option<String>,
    #[serde(default)]
    pub capture_method: Option<String>,
    /// How much of the original target can be restored. `exact_ax` means the
    /// AX control and UTF-16 selection are available; `foreground_paste` means
    /// only the original foreground window is known and delivery uses Cmd+V.
    #[serde(default)]
    pub capability: Option<String>,
    pub captured_at: String,
    pub available: bool,
    pub selection_location: i64,
    pub selection_length: i64,
    pub caret_x: Option<f64>,
    pub caret_y: Option<f64>,
    pub window_bounds: Option<[f64; 4]>,
    pub caret_offset: Option<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasteItem {
    pub id: String,
    pub seq: i64,
    pub raw_text: String,
    pub text: String,
    pub processing_status: String,
    pub delivery_status: String,
    pub mode: PasteMode,
    pub target: Option<PasteTarget>,
    pub created_at: String,
    pub version: i64,
    pub actioned: bool,
    pub error: Option<String>,
    pub polish_error: Option<String>,
    pub notification_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasteboxState {
    pub notifications_enabled: bool,
    pub items: Vec<PasteItem>,
    pub pending_count: i64,
    pub mode: PasteMode,
    pub targets: Vec<PasteTarget>,
    pub opening_target_id: Option<String>,
    pub requested_item_id: Option<String>,
    pub requested_item: Option<PasteItem>,
    pub target_error: Option<String>,
    pub notification_status: String,
    pub accessibility_trusted: bool,
    pub backend_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasteReceipt {
    pub verified: bool,
    pub message: String,
}
