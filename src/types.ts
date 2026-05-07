export interface RecordingInfo {
  is_recording: boolean;
  duration: number;
  path?: string;
  recording_mode: string;
  system_audio_available: boolean;
}

export interface Recording {
  id: string;
  name: string;
  path: string;
  created: string;
  size: number;
}

export interface NoteDoc {
  id: string;
  title: string;
  content: string;
  created: string;
  updated: string;
}

export type RightPaneTab = 'recordings' | 'notes' | 'jobs' | 'assistant';

export interface LocalWorkspace {
  id: string;
  type: 'local';
  title: string;
  folderName: string;
  path?: string;
  created: string;
  updated: string;
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
  metadata?: Record<string, any> | null;
}

export type AssistantScope = 'current' | 'global';

export interface AssistantSource {
  type?: 'note' | 'web' | null;
  id: string;
  note_id: string;
  title: string;
  workspace_folder?: string | null;
  url?: string | null;
  snippet?: string | null;
  start_line?: number | null;
  end_line?: number | null;
}

export interface AssistantSession {
  id: string;
  title: string;
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

export interface AssistantMessage {
  id: string;
  session_id: string;
  role: 'user' | 'assistant';
  content: string;
  scope: AssistantScope;
  workspace_folder?: string | null;
  provider?: string | null;
  model?: string | null;
  sources: AssistantSource[];
  created_at: string;
}

export interface AssistantRun {
  request_id: string;
  session_id: string;
  status: 'running' | 'completed' | 'failed';
  scope: AssistantScope;
  workspace_folder: string;
  question: string;
  prompt_template_id?: string | null;
  web_enabled: boolean;
  max_turns: string;
  current_turn: number;
  partial_answer: string;
  error_message?: string | null;
  provider?: string | null;
  model?: string | null;
  created_at: string;
  updated_at: string;
  finished_at?: string | null;
}

export interface AssistantWorkspaceActivity {
  active_run?: AssistantRun | null;
  latest_session_id?: string | null;
}

export interface AssistantAskResult {
  session: AssistantSession;
  user_message: AssistantMessage;
  assistant_message: AssistantMessage;
}

export interface AssistantStreamEvent {
  request_id: string;
  event: 'started' | 'delta' | 'tool' | 'turn' | 'done' | 'error';
  text?: string | null;
  name?: string | null;
  sources?: AssistantSource[] | null;
  session?: AssistantSession | null;
  run?: AssistantRun | null;
  user_message?: AssistantMessage | null;
  assistant_message?: AssistantMessage | null;
  error?: string | null;
  current_turn?: number | null;
  max_turns?: string | number | null;
}
