export interface LocalRecording {
  id: string;
  title: string;
  original_audio_path: string;
  normalized_audio_path?: string | null;
  duration_ms?: number | null;
  file_size_bytes: number;
  created_at: string;
  updated_at: string;
  transcription_status: string;
  summary_status: string;
  latest_transcript_id?: string | null;
  latest_summary_id?: string | null;
}

export interface LocalQueueJob {
  id: string;
  queue_type: 'transcription' | 'summary' | string;
  status: string;
  progress: number;
  title: string;
  subtitle?: string | null;
  workspace_folder?: string | null;
  created_at: string;
  queued_at?: string | null;
  started_at?: string | null;
  finished_at?: string | null;
  updated_at?: string | null;
  error_message?: string | null;
  retry_count: number;
  recording_id?: string | null;
  transcript_id?: string | null;
  template_id?: string | null;
  summary_scope?: string | null;
  output_transcript_id?: string | null;
  output_summary_id?: string | null;
  metadata?: LocalQueueJobMetadata | null;
}

export interface LocalQueueJobMetadata {
  pipeline?: LocalJobPipeline | null;
  [key: string]: any;
}

export interface LocalJobPipeline {
  steps?: LocalJobPipelineStep[];
  current_step_index?: number | null;
  total_progress?: number;
  total_duration_ms?: number;
  started_at?: string | null;
  completed_at?: string | null;
}

export interface LocalJobPipelineStep {
  name: string;
  display_name?: string;
  status: 'pending' | 'running' | 'completed' | 'failed' | 'skipped' | string;
  progress?: number;
  optional?: boolean;
  started_at?: string | null;
  completed_at?: string | null;
  duration_ms?: number | null;
  metadata?: Record<string, any>;
  error?: { message?: string } | null;
}

export interface TranscriptRecord {
  id: string;
  recording_id: string;
  job_id: string;
  text: string;
  result_json: string;
  language?: string | null;
  confidence?: number | null;
  duration_ms?: number | null;
  rtf?: number | null;
  created_at: string;
}

export interface SummaryTemplate {
  id: string;
  name: string;
  description?: string | null;
  prompt: string;
  is_builtin: boolean;
  created_at: string;
  updated_at: string;
}

export interface AssistantPromptTemplate {
  id: string;
  name: string;
  description?: string | null;
  prompt: string;
  is_builtin: boolean;
  created_at: string;
  updated_at: string;
}

export interface SummaryRecord {
  id: string;
  transcript_id?: string | null;
  job_id: string;
  template_id: string;
  title?: string | null;
  content: string;
  result_json?: string | null;
  provider: string;
  model: string;
  created_at: string;
  updated_at: string;
}

export interface TranscriptionJobResult {
  job_id: string;
  transcript: TranscriptRecord;
}

export interface SummaryJobResult {
  job_id: string;
  summary: SummaryRecord;
}

export interface WorkspaceSummaryResult {
  job_id: string;
  summary: SummaryRecord;
  content: string;
}

export interface AppSettings {
  python_path: string;
  ffmpeg_path: string;
  asr_model_repo: string;
  asr_model_source: string;
  asr_model_path?: string | null;
  http_proxy?: string | null;
  https_proxy?: string | null;
  all_proxy?: string | null;
  use_gpu: boolean;
  llm_provider: string;
  llm_base_url: string;
  llm_model: string;
  llm_temperature: number;
  llm_max_tokens: number;
  llm_timeout_seconds: number;
  has_llm_api_key: boolean;
  voice_input_enabled: boolean;
  voice_input_hotkey: string;
  voice_input_refinement_mode: 'local' | 'ai_polish' | string;
  voice_input_refinement_prompt: string;
}

export interface VoiceInputStats {
  today_success_count: number;
  today_success_chars: number;
  total_success_count: number;
  total_success_chars: number;
  last_success_at?: string | null;
  last_success_chars: number;
}

export interface VoiceInputPermissionStatus {
  platform: string;
  microphone_ok: boolean;
  microphone_message: string;
  accessibility_ok: boolean;
  accessibility_message: string;
}

export type VoiceInputPhase =
  | 'idle'
  | 'starting'
  | 'listening'
  | 'preparing_model'
  | 'transcribing'
  | 'refining'
  | 'inserting'
  | 'inserted'
  | 'copied'
  | 'failed'
  | 'cancelled'
  | string;

export interface VoiceInputStatus {
  phase: VoiceInputPhase;
  message: string;
  started_at?: string | null;
}

export interface VoiceInputStatusEvent {
  phase: VoiceInputPhase;
  message: string;
  char_count?: number | null;
  insertion_strategy?: string | null;
}

export type VoiceInputWarmupPhase = 'warming' | 'ready' | 'skipped' | string;

export interface VoiceInputWarmupStatusEvent {
  phase: VoiceInputWarmupPhase;
  message: string;
  reason: string;
  elapsed_ms?: number | null;
  sidecar_infer_ms?: number | null;
}

export interface VoiceInputDictationResult {
  raw_text: string;
  text: string;
  inserted: boolean;
  insertion_strategy: string;
  message: string;
  polish_fallback: boolean;
  polish_error?: string | null;
  stats: VoiceInputStats;
}

export interface RuntimeDependencyStatus {
  python_ok: boolean;
  python_message: string;
  ffmpeg_ok: boolean;
  ffmpeg_message: string;
}

export interface ModelStatus {
  repo: string;
  status: string;
  path?: string | null;
  message?: string | null;
  updated_at?: string | null;
}

export interface ModelDownloadProgress {
  repo: string;
  source: string;
  status: string;
  downloaded_bytes: number;
  total_bytes?: number | null;
  speed_bytes_per_second: number;
  percent?: number | null;
  message?: string | null;
}
