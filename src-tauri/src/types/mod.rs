pub mod models;
mod pastebox;
pub use pastebox::*;

// Re-export all types
pub use models::{
    AppSettings, AssistantAskResult, AssistantMessage, AssistantPromptTemplate, AssistantRun,
    AssistantSession, AssistantSource, AssistantWorkspaceActivity, ImportedNote, LocalNote,
    LocalQueueJob, LocalRecording, ModelDownloadProgress, ModelStatus, RecordingArgs,
    RecordingFile, RecordingInfo, RuntimeDependencyStatus, SaveSettingsRequest, SummaryJobResult,
    SummaryRecord, SummaryTemplate, TranscriptRecord, TranscriptionJobResult,
    VoiceInputDictationResult, VoiceInputInsertionResult, VoiceInputPermissionStatus,
    VoiceInputStats, VoiceInputStatus, VoiceInputStatusEvent, VoiceInputSubmission,
    VoiceInputWarmupStatusEvent, WorkspaceSummaryResult, WorkspaceTextDocument,
};
