use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct RecordingArgs {
    pub save_path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct RecordingInfo {
    pub is_recording: bool,
    pub duration: f64,
    pub path: Option<String>,
    pub recording_mode: String,
    pub system_audio_available: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct RecordingFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub created: String,
    pub size: u64,
}

#[derive(Debug, Serialize)]
pub struct ImportedNote {
    pub title: String,
    pub content: String,
    pub file_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalNote {
    pub id: String,
    pub title: String,
    pub content: String,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalRecording {
    pub id: String,
    pub title: String,
    pub original_audio_path: String,
    pub normalized_audio_path: Option<String>,
    pub duration_ms: Option<i64>,
    pub file_size_bytes: i64,
    pub created_at: String,
    pub updated_at: String,
    pub transcription_status: String,
    pub summary_status: String,
    pub latest_transcript_id: Option<String>,
    pub latest_summary_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalQueueJob {
    pub id: String,
    pub queue_type: String,
    pub status: String,
    pub progress: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub workspace_folder: Option<String>,
    pub created_at: String,
    pub queued_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub updated_at: Option<String>,
    pub error_message: Option<String>,
    pub retry_count: i64,
    pub recording_id: Option<String>,
    pub transcript_id: Option<String>,
    pub template_id: Option<String>,
    pub summary_scope: Option<String>,
    pub output_transcript_id: Option<String>,
    pub output_summary_id: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptRecord {
    pub id: String,
    pub recording_id: String,
    pub job_id: String,
    pub text: String,
    pub result_json: String,
    pub language: Option<String>,
    pub confidence: Option<f64>,
    pub duration_ms: Option<i64>,
    pub rtf: Option<f64>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SummaryTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub prompt: String,
    pub is_builtin: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SummaryRecord {
    pub id: String,
    pub transcript_id: Option<String>,
    pub job_id: String,
    pub template_id: String,
    pub title: Option<String>,
    pub content: String,
    pub result_json: Option<String>,
    pub provider: String,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub python_path: String,
    pub ffmpeg_path: String,
    pub asr_model_repo: String,
    pub asr_model_source: String,
    pub asr_model_path: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub all_proxy: Option<String>,
    pub use_gpu: bool,
    pub llm_provider: String,
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_temperature: f64,
    pub llm_max_tokens: i64,
    pub llm_timeout_seconds: i64,
    pub has_llm_api_key: bool,
    pub voice_input_enabled: bool,
    pub voice_input_hotkey: String,
    pub voice_input_refinement_mode: String,
    pub voice_input_refinement_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SaveSettingsRequest {
    pub python_path: String,
    pub ffmpeg_path: String,
    pub asr_model_repo: String,
    pub asr_model_source: String,
    pub asr_model_path: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub all_proxy: Option<String>,
    pub use_gpu: bool,
    pub llm_provider: String,
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_temperature: f64,
    pub llm_max_tokens: i64,
    pub llm_timeout_seconds: i64,
    pub voice_input_enabled: bool,
    pub voice_input_hotkey: String,
    pub voice_input_refinement_mode: String,
    pub voice_input_refinement_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceInputStats {
    pub today_success_count: i64,
    pub today_success_chars: i64,
    pub total_success_count: i64,
    pub total_success_chars: i64,
    pub last_success_at: Option<String>,
    pub last_success_chars: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceInputPermissionStatus {
    pub platform: String,
    pub microphone_ok: bool,
    pub microphone_message: String,
    pub accessibility_ok: bool,
    pub accessibility_message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceInputStatus {
    pub phase: String,
    pub message: String,
    pub started_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceInputStatusEvent {
    pub phase: String,
    pub message: String,
    pub char_count: Option<i64>,
    pub insertion_strategy: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceInputWarmupStatusEvent {
    pub phase: String,
    pub message: String,
    pub reason: String,
    pub elapsed_ms: Option<i64>,
    pub sidecar_infer_ms: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceInputInsertionResult {
    pub strategy: String,
    pub inserted: bool,
    pub clipboard_left_text: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceInputDictationResult {
    pub raw_text: String,
    pub text: String,
    pub inserted: bool,
    pub insertion_strategy: String,
    pub message: String,
    pub polish_fallback: bool,
    pub polish_error: Option<String>,
    pub stats: VoiceInputStats,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelStatus {
    pub repo: String,
    pub status: String,
    pub path: Option<String>,
    pub message: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelDownloadProgress {
    pub repo: String,
    pub source: String,
    pub status: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed_bytes_per_second: f64,
    pub percent: Option<f64>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RuntimeDependencyStatus {
    pub python_ok: bool,
    pub python_message: String,
    pub ffmpeg_ok: bool,
    pub ffmpeg_message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptionJobResult {
    pub job_id: String,
    pub transcript: TranscriptRecord,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SummaryJobResult {
    pub job_id: String,
    pub summary: SummaryRecord,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceSummaryResult {
    pub job_id: String,
    pub summary: SummaryRecord,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceTextDocument {
    pub title: String,
    pub content: String,
}

fn default_assistant_source_type() -> Option<String> {
    Some("note".to_string())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantSource {
    #[serde(default = "default_assistant_source_type", rename = "type")]
    pub source_type: Option<String>,
    pub id: String,
    #[serde(default)]
    pub note_id: String,
    pub title: String,
    #[serde(default)]
    pub workspace_folder: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub start_line: Option<u64>,
    #[serde(default)]
    pub end_line: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantSession {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantPromptTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub prompt: String,
    pub is_builtin: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub scope: String,
    pub workspace_folder: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub sources: Vec<AssistantSource>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantRun {
    pub request_id: String,
    pub session_id: String,
    pub status: String,
    pub scope: String,
    pub workspace_folder: String,
    pub question: String,
    pub prompt_template_id: Option<String>,
    pub web_enabled: bool,
    pub max_turns: String,
    pub current_turn: i64,
    pub partial_answer: String,
    pub error_message: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantWorkspaceActivity {
    pub active_run: Option<AssistantRun>,
    pub latest_session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantAskResult {
    pub session: AssistantSession,
    pub user_message: AssistantMessage,
    pub assistant_message: AssistantMessage,
}
