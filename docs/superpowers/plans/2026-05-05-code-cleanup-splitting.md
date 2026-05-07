# Code Cleanup Splitting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split oversized Rust, React, and Python files into focused modules without changing runtime behavior.

**Architecture:** Keep React as the application coordinator and move Tauri IPC access behind service or command boundaries when touching frontend code. Keep Rust commands' exported function names stable so `src-tauri/src/commands/mod.rs` and Tauri handler registration do not need broad changes. Keep Python sidecar stdio protocols stable and split internals behind the existing `main.py` entrypoints.

**Tech Stack:** Tauri 2, Rust, React 18, TypeScript, Vite, Node `.mjs` contract tests, Python sidecars.

---

## Current Preconditions

- Worktree currently has uncommitted changes in:
  - `src-tauri/src/voice_input/overlay.rs`
  - `src-tauri/tauri.conf.json`
  - `src/components/VoiceInputOverlay.css`
  - `tests/voice-input-overlay-contract.test.mjs`
- Do not mix those files into cleanup commits unless the task explicitly owns them.
- Current largest cleanup targets:
  - `src-tauri/src/commands/local.rs` - 1788 lines
  - `src/components/RecordingSettingsTab.tsx` - 1289 lines
  - `src/components/AssistantPanel.tsx` - 1114 lines
  - `src-tauri/src/commands/assistant.rs` - 1108 lines
  - `src-tauri/src/voice_input/mod.rs` - 1082 lines
  - `sidecars/funasr_nano_mlx/main.py` - 1069 lines
- Additional cleanup targets after the first pass:
  - `src-tauri/src/tests/db_queue.rs` - 924 lines
  - `src-tauri/src/sidecar.rs` - 847 lines
  - `src-tauri/src/local_queue.rs` - 766 lines
  - `src-tauri/src/audio/core.rs` - 765 lines
  - `src/components/RecordingList.tsx` - 747 lines
  - `sidecars/local_notes_agent/main.py` - 679 lines
  - `src-tauri/src/files/manager.rs` - 647 lines
  - `src-tauri/src/llm.rs` - 577 lines
  - `src/lib/workspace.ts` - 542 lines
  - `sidecars/local_notes_agent/notes_mcp_server.py` - 507 lines

## Priority Order

1. P0: Split `src-tauri/src/commands/local.rs`
2. P1: Split `src-tauri/src/commands/assistant.rs`
3. P2: Split `src-tauri/src/voice_input/mod.rs`
4. P3: Split `src/components/RecordingSettingsTab.tsx` and related CSS
5. P4: Split `src/components/AssistantPanel.tsx`, `src/components/RecordingList.tsx`, and related CSS
6. P5: Split Python sidecars, including MCP helper servers
7. P6: Split remaining Rust infrastructure files
8. P7: Split frontend workspace services and remove component-level Tauri IPC
9. P8: Split oversized Rust and Python test files

---

### Task 0: Preflight And Commit Hygiene

**Files:**
- Inspect only: `git status --short`
- Do not modify unrelated dirty files.

- [ ] **Step 1: Confirm dirty worktree**

Run:
```bash
git status --short
```

Expected:
```text
 M src-tauri/src/voice_input/overlay.rs
 M src-tauri/tauri.conf.json
 M src/components/VoiceInputOverlay.css
 M tests/voice-input-overlay-contract.test.mjs
```

- [ ] **Step 2: Decide isolation**

If these changes are still present, either commit them separately or leave them unstaged during cleanup commits.

- [ ] **Step 3: Baseline verification**

Run:
```bash
cargo check
cargo test --lib
npm run build
```

Expected: all commands pass. If `npm run build` fails because of existing dirty overlay changes, stop and resolve that separately before refactoring.

---

### Task 1: Split Rust Local Commands

**Why first:** This is the largest file and has the best backend test coverage. It is mostly command grouping and helper extraction, so it has high maintainability gain with manageable behavior risk.

**Files:**
- Move: `src-tauri/src/commands/local.rs` -> `src-tauri/src/commands/local/mod.rs`
- Create: `src-tauri/src/commands/local/recordings.rs`
- Create: `src-tauri/src/commands/local/settings.rs`
- Create: `src-tauri/src/commands/local/templates.rs`
- Create: `src-tauri/src/commands/local/models.rs`
- Create: `src-tauri/src/commands/local/transcription.rs`
- Create: `src-tauri/src/commands/local/summaries.rs`
- Create: `src-tauri/src/commands/local/queue.rs`
- Create: `src-tauri/src/commands/local/exports.rs`
- Verify unchanged public exports from: `src-tauri/src/commands/mod.rs`

- [ ] **Step 1: Move file to module facade**

Move `src-tauri/src/commands/local.rs` to `src-tauri/src/commands/local/mod.rs`.

- [ ] **Step 2: Add child module declarations**

At the top of `src-tauri/src/commands/local/mod.rs`, declare the extracted modules and re-export public command functions:

```rust
mod exports;
mod models;
mod queue;
mod recordings;
mod settings;
mod summaries;
mod templates;
mod transcription;

pub use exports::{export_summary, export_transcript};
pub use models::{
    cancel_asr_model_download, check_runtime_dependencies, ensure_asr_model,
    get_model_download_progress, get_model_status,
};
pub use queue::{
    cancel_local_queue_job, enqueue_summary, enqueue_transcription, enqueue_workspace_summary,
    enqueue_workspace_text_summary, list_local_queue_jobs, mark_local_queue_job_synced,
};
pub use recordings::{delete_recording, list_recordings_with_status, register_recording};
pub use settings::{get_settings, has_llm_api_key, save_settings, set_llm_api_key, test_llm_provider};
pub use summaries::{
    get_summary, retry_summary, summarize_transcript, summarize_workspace_texts,
    summarize_workspace_transcripts,
};
pub use templates::{delete_summary_template, list_summary_templates, save_summary_template};
pub use transcription::{get_latest_transcript, get_transcript, retry_transcription, transcribe_recording};
```

- [ ] **Step 3: Extract modules one at a time**

Move only related functions per module. Use `pub(super)` for helpers shared only inside `commands::local`, and keep Tauri commands as `pub async fn` or `pub fn` with their existing names and signatures.

- [ ] **Step 4: Verify Rust**

Run:
```bash
cargo fmt
cargo check
cargo test --lib
```

Expected: zero Rust warnings and all library tests pass.

- [ ] **Step 5: Commit**

Run:
```bash
git add src-tauri/src/commands/local.rs src-tauri/src/commands/local
git commit -m "refactor: split local command modules"
```

---

### Task 2: Split Rust Assistant Commands

**Why second:** `assistant.rs` mixes command handlers, request validation, prompt construction, markdown export, and streaming events. Splitting it reduces risk before assistant behavior changes.

**Files:**
- Move: `src-tauri/src/commands/assistant.rs` -> `src-tauri/src/commands/assistant/mod.rs`
- Create: `src-tauri/src/commands/assistant/sessions.rs`
- Create: `src-tauri/src/commands/assistant/messages.rs`
- Create: `src-tauri/src/commands/assistant/templates.rs`
- Create: `src-tauri/src/commands/assistant/request_builder.rs`
- Create: `src-tauri/src/commands/assistant/validation.rs`
- Create: `src-tauri/src/commands/assistant/notes.rs`
- Create: `src-tauri/src/commands/assistant/stream_events.rs`
- Verify unchanged public exports from: `src-tauri/src/commands/mod.rs`

- [ ] **Step 1: Move file to module facade**

Move `src-tauri/src/commands/assistant.rs` to `src-tauri/src/commands/assistant/mod.rs`.

- [ ] **Step 2: Extract pure helpers first**

Extract validation and request-building helpers before moving command handlers. This keeps the public command surface stable while reducing file size.

- [ ] **Step 3: Extract command groups**

Move session commands to `sessions.rs`, message commands to `messages.rs`, prompt template commands to `templates.rs`, note export commands to `notes.rs`, and streaming emitters to `stream_events.rs`.

- [ ] **Step 4: Verify Rust and assistant contracts**

Run:
```bash
cargo fmt
cargo check
cargo test --lib
node tests/assistant-workspace-service.test.mjs
node tests/assistant-panel-ui.test.mjs
node tests/assistant-web-notice.test.mjs
```

Expected: all pass with unchanged assistant command names.

- [ ] **Step 5: Commit**

Run:
```bash
git add src-tauri/src/commands/assistant.rs src-tauri/src/commands/assistant
git commit -m "refactor: split assistant command modules"
```

---

### Task 3: Split Voice Input Runtime

**Why third:** `voice_input/mod.rs` is one of the largest files and currently mixes state transitions, hotkey command routing, short-recording lifecycle, ASR/refinement, event emission, and platform permission helpers. Split this before frontend voice-input cleanup so the backend command surface is easier to reason about.

**Files:**
- Keep facade: `src-tauri/src/voice_input/mod.rs`
- Do not modify dirty file: `src-tauri/src/voice_input/overlay.rs`
- Existing modules remain:
  - `src-tauri/src/voice_input/hotkey.rs`
  - `src-tauri/src/voice_input/insertion.rs`
  - `src-tauri/src/voice_input/platform_hotkey.rs`
  - `src-tauri/src/voice_input/recorder.rs`
  - `src-tauri/src/voice_input/text.rs`
  - `src-tauri/src/voice_input/warmup.rs`
- Create: `src-tauri/src/voice_input/state.rs`
- Create: `src-tauri/src/voice_input/events.rs`
- Create: `src-tauri/src/voice_input/permissions.rs`
- Create: `src-tauri/src/voice_input/hotkey_runtime.rs`
- Create: `src-tauri/src/voice_input/dictation.rs`
- Create: `src-tauri/src/voice_input/asr.rs`
- Verify unchanged command wrappers from: `src-tauri/src/commands/voice_input.rs`

- [ ] **Step 1: Turn `mod.rs` into a module facade**

Keep existing public module declarations and add the new private modules. Re-export the same public API currently provided by `voice_input::mod`:

```rust
pub mod hotkey;
pub mod insertion;
mod overlay;
mod platform_hotkey;
mod recorder;
pub mod text;
mod warmup;

mod asr;
mod dictation;
mod events;
mod hotkey_runtime;
mod permissions;
mod state;

#[cfg(test)]
pub(crate) use overlay::overlay_position_for_work_area;

pub use dictation::{cancel_dictation, init, start_dictation, stop_dictation, toggle_dictation};
pub use hotkey_runtime::{apply_hotkey_registration, reload_hotkey_registration};
pub use permissions::{permission_status, request_accessibility_permission};
pub use state::status;

#[cfg(test)]
pub(crate) use permissions::accessibility_permission_hint;
```

- [ ] **Step 2: Extract state and event helpers**

Move `VoiceInputPhase`, `VoiceInputState`, `STATE`, `status()`, idle/listening checks, phase mutation, and status message formatting into `state.rs`. Move `emit_status()` and the Tauri `voice-input-status` event shaping into `events.rs`. Keep helper visibility to `pub(super)` unless existing tests or command wrappers require wider access.

- [ ] **Step 3: Extract permission helpers**

Move `permission_status()`, `request_accessibility_permission()`, `accessibility_permission_hint()`, `accessibility_trusted()`, and `request_accessibility_trust_prompt()` into `permissions.rs`. Preserve the current macOS and non-macOS `cfg` behavior and exact user-facing strings.

- [ ] **Step 4: Extract hotkey runtime**

Move `HotkeyCommand`, `HOTKEY_COMMAND_TX`, `HOTKEY_WATCHER`, `run_hotkey_registration()`, `apply_hotkey_settings()`, `send_hotkey_command()`, `notify_hotkey_triggered()`, and `notify_enter_pressed()` into `hotkey_runtime.rs`. Keep `platform_hotkey.rs` unchanged unless compilation requires a visibility-only adjustment.

- [ ] **Step 5: Extract dictation lifecycle and ASR pipeline**

Move `init()`, `STARTUP_WARMUP`, `start_dictation()`, `stop_dictation()`, `cancel_dictation()`, and `toggle_dictation()` into `dictation.rs`. Move `MIN_AUDIO_SAMPLES`, `VOICE_INPUT_ASR_TIMEOUT_SECS`, `VoiceInputPolishOutcome`, `process_samples()`, `transcribe_short_audio()`, and refinement helpers into `asr.rs`.

- [ ] **Step 6: Verify Rust and voice input contracts**

Run:
```bash
cargo fmt
cargo check
cargo test --lib
node tests/voice-input-overlay-contract.test.mjs
```

Expected: all pass with unchanged Tauri command names and unchanged voice input status event payloads.

- [ ] **Step 7: Commit**

Run:
```bash
git add src-tauri/src/voice_input/mod.rs src-tauri/src/voice_input/state.rs src-tauri/src/voice_input/events.rs src-tauri/src/voice_input/permissions.rs src-tauri/src/voice_input/hotkey_runtime.rs src-tauri/src/voice_input/dictation.rs src-tauri/src/voice_input/asr.rs
git commit -m "refactor: split voice input runtime"
```

---

### Task 4: Split Recording Settings UI

**Why fourth:** This is the largest frontend component. It contains UI rendering, settings load/save, hotkey capture, model status polling, permissions, and field rendering in one place.

**Files:**
- Keep facade: `src/components/RecordingSettingsTab.tsx`
- Keep shared shell styles: `src/components/RecordingSettingsTab.css`
- Create: `src/components/recording-settings/AudioDeviceSection.tsx`
- Create: `src/components/recording-settings/VoiceInputSection.tsx`
- Create: `src/components/recording-settings/AsrModelSection.tsx`
- Create: `src/components/recording-settings/LlmProviderSection.tsx`
- Create: `src/components/recording-settings/SummaryTemplateSection.tsx`
- Create: `src/components/recording-settings/SettingsField.tsx`
- Create: `src/components/recording-settings/useRecordingSettings.ts`
- Create: `src/components/recording-settings/useVoiceInputHotkeyCapture.ts`
- Create: `src/components/recording-settings/types.ts`
- Create: `src/components/recording-settings/recording-settings-sections.css`

- [ ] **Step 1: Extract types and pure formatting helpers**

Move local prop types, option arrays, status labels, and display helpers to `types.ts` or small module-local helpers. Do not change rendered text.

- [ ] **Step 2: Extract hotkey capture hook**

Move hotkey capture state and event handling to `useVoiceInputHotkeyCapture.ts`. Keep existing tests focused on `tests/hotkey-capture-ui.test.mjs` and `tests/voice-input-hotkey-linkage.test.mjs`.

- [ ] **Step 3: Extract model and provider sections**

Move ASR model status/download UI and LLM provider/key testing UI into section components. Pass callbacks and state through typed props; do not call Tauri directly from new section components.

- [ ] **Step 4: Extract section CSS**

Move section-specific rules from `RecordingSettingsTab.css` into `recording-settings-sections.css`. Keep modal layout and shared tab shell rules in `RecordingSettingsTab.css`; import the new CSS from `RecordingSettingsTab.tsx` or the section facade.

- [ ] **Step 5: Verify frontend contracts**

Run:
```bash
node tests/settings-save-actions.test.mjs
node tests/hotkey-capture-ui.test.mjs
node tests/voice-input-hotkey-linkage.test.mjs
npm run build
```

Expected: all tests and TypeScript build pass.

- [ ] **Step 6: Commit**

Run:
```bash
git add src/components/RecordingSettingsTab.tsx src/components/RecordingSettingsTab.css src/components/recording-settings tests/settings-save-actions.test.mjs tests/hotkey-capture-ui.test.mjs tests/voice-input-hotkey-linkage.test.mjs
git commit -m "refactor: split recording settings UI"
```

---

### Task 5: Split Assistant Panel UI

**Why fifth:** This reduces frontend assistant complexity after the Rust assistant command layer is already stable.

**Files:**
- Keep facade: `src/components/AssistantPanel.tsx`
- Keep shared shell styles: `src/components/AssistantPanel.css`
- Create: `src/components/assistant-panel/AssistantHeader.tsx`
- Create: `src/components/assistant-panel/AssistantMessageList.tsx`
- Create: `src/components/assistant-panel/AssistantMessageBubble.tsx`
- Create: `src/components/assistant-panel/AssistantComposer.tsx`
- Create: `src/components/assistant-panel/AssistantSources.tsx`
- Create: `src/components/assistant-panel/useAssistantSessions.ts`
- Create: `src/components/assistant-panel/useAssistantStreaming.ts`
- Create: `src/components/assistant-panel/messageMerge.ts`
- Create: `src/components/assistant-panel/storage.ts`
- Create: `src/components/assistant-panel/types.ts`
- Create: `src/components/assistant-panel/assistant-panel-sections.css`

- [ ] **Step 1: Extract pure message and storage helpers**

Move message merging, pending state helpers, local storage keys, and option normalization before moving JSX.

- [ ] **Step 2: Extract rendering components**

Move message list, bubble, composer, header, and sources into focused components. Keep `AssistantPanel.tsx` responsible for orchestration and state wiring.

- [ ] **Step 3: Extract assistant panel CSS**

Move message list, bubble, composer, source, and header rules from `AssistantPanel.css` into `assistant-panel-sections.css`. Keep outer panel layout rules in `AssistantPanel.css`.

- [ ] **Step 4: Verify assistant UI**

Run:
```bash
node tests/assistant-panel-ui.test.mjs
node tests/assistant-workspace-service.test.mjs
node tests/assistant-web-notice.test.mjs
npm run build
```

Expected: all pass.

- [ ] **Step 5: Commit**

Run:
```bash
git add src/components/AssistantPanel.tsx src/components/AssistantPanel.css src/components/assistant-panel tests/assistant-panel-ui.test.mjs tests/assistant-workspace-service.test.mjs tests/assistant-web-notice.test.mjs
git commit -m "refactor: split assistant panel UI"
```

---

### Task 6: Split Recording List UI

**Files:**
- Keep facade: `src/components/RecordingList.tsx`
- Keep shared shell styles: `src/components/RecordingList.css`
- Create: `src/components/recording-list/RecordingListHeader.tsx`
- Create: `src/components/recording-list/RecordingListItem.tsx`
- Create: `src/components/recording-list/RecordingStatusBadge.tsx`
- Create: `src/components/recording-list/RecordingActions.tsx`
- Create: `src/components/recording-list/useRecordingListActions.ts`
- Create: `src/components/recording-list/types.ts`
- Create: `src/components/recording-list/recording-list-sections.css`

- [ ] **Step 1: Extract item rendering**

Move repeated row/card rendering into `RecordingListItem.tsx` with explicit props.

- [ ] **Step 2: Extract action handlers**

Move delete, retry, export, open, and status refresh behavior into `useRecordingListActions.ts`. Keep service calls in hooks, not leaf UI components.

- [ ] **Step 3: Extract recording list CSS**

Move row, status badge, action, and list header rules from `RecordingList.css` into `recording-list-sections.css`. Keep container layout and sidebar integration rules in `RecordingList.css`.

- [ ] **Step 4: Verify**

Run:
```bash
npm run build
```

Expected: TypeScript and Vite build pass.

- [ ] **Step 5: Commit**

Run:
```bash
git add src/components/RecordingList.tsx src/components/RecordingList.css src/components/recording-list
git commit -m "refactor: split recording list UI"
```

---

### Task 7: Split FunASR Python Sidecar

**Files:**
- Keep entrypoint: `sidecars/funasr_nano_mlx/main.py`
- Create: `sidecars/funasr_nano_mlx/protocol.py`
- Create: `sidecars/funasr_nano_mlx/audio_io.py`
- Create: `sidecars/funasr_nano_mlx/models.py`
- Create: `sidecars/funasr_nano_mlx/transcript.py`
- Create: `sidecars/funasr_nano_mlx/handlers.py`

- [ ] **Step 1: Extract protocol types and JSON IO**

Move request/response shaping and stdio JSON helpers to `protocol.py`. Keep `main.py` as the process entrypoint.

- [ ] **Step 2: Extract model download/load/cache**

Move model path resolution, download progress, model cache, warmup, and generation logic to `models.py`.

- [ ] **Step 3: Extract transcript formatting**

Move timestamp normalization, segment formatting, and transcript text generation to `transcript.py`.

- [ ] **Step 4: Verify sidecar tests**

Run:
```bash
runtime/asr/bin/python -m pytest tests/sidecars/funasr_nano_mlx/test_pipeline.py
```

Expected: all FunASR sidecar tests pass.

- [ ] **Step 5: Commit**

Run:
```bash
git add sidecars/funasr_nano_mlx tests/sidecars/funasr_nano_mlx/test_pipeline.py
git commit -m "refactor: split funasr sidecar modules"
```

---

### Task 8: Split Local Notes Agent Python Sidecar

**Files:**
- Keep entrypoint: `sidecars/local_notes_agent/main.py`
- Keep entrypoint: `sidecars/local_notes_agent/web_mcp_server.py`
- Keep entrypoint: `sidecars/local_notes_agent/notes_mcp_server.py`
- Create: `sidecars/local_notes_agent/protocol.py`
- Create: `sidecars/local_notes_agent/workspace.py`
- Create: `sidecars/local_notes_agent/llm_client.py`
- Create: `sidecars/local_notes_agent/search_tools.py`
- Create: `sidecars/local_notes_agent/handlers.py`
- Create: `sidecars/local_notes_agent/mcp_protocol.py`
- Create: `sidecars/local_notes_agent/notes_store.py`

- [ ] **Step 1: Extract protocol and handlers**

Keep `main.py` responsible for stdio loop startup. Move command dispatch to `handlers.py` and request/response structures to `protocol.py`.

- [ ] **Step 2: Extract service clients**

Move workspace loading/search into `workspace.py`, LLM calls into `llm_client.py`, and web/search helpers into `search_tools.py`.

- [ ] **Step 3: Extract MCP helper servers**

Move shared JSON-RPC request/response helpers from `web_mcp_server.py` and `notes_mcp_server.py` into `mcp_protocol.py`. Move `NoteFile`, note indexing, note matching, and notes tool execution from `notes_mcp_server.py` into `notes_store.py`. Keep both MCP server files as executable entrypoints.

- [ ] **Step 4: Verify sidecar tests**

Run:
```bash
runtime/asr/bin/python -m pytest tests/sidecars/local_notes_agent/test_main.py tests/sidecars/local_notes_agent/test_web_mcp_server.py
```

Expected: all local notes agent tests pass.

- [ ] **Step 5: Commit**

Run:
```bash
git add sidecars/local_notes_agent tests/sidecars/local_notes_agent
git commit -m "refactor: split local notes agent modules"
```

---

### Task 9: Split Remaining Rust Infrastructure

**Files:**
- Split: `src-tauri/src/sidecar.rs`
- Split: `src-tauri/src/local_queue.rs`
- Split: `src-tauri/src/audio/core.rs`
- Split: `src-tauri/src/files/manager.rs`
- Split: `src-tauri/src/llm.rs`

- [ ] **Step 1: Split sidecar process management**

Target modules:
```text
src-tauri/src/sidecar/mod.rs
src-tauri/src/sidecar/process.rs
src-tauri/src/sidecar/protocol.rs
src-tauri/src/sidecar/runtime.rs
src-tauri/src/sidecar/errors.rs
```

- [ ] **Step 2: Split local queue**

Target modules:
```text
src-tauri/src/local_queue/mod.rs
src-tauri/src/local_queue/jobs.rs
src-tauri/src/local_queue/store.rs
src-tauri/src/local_queue/worker.rs
src-tauri/src/local_queue/types.rs
```

- [ ] **Step 3: Split audio core**

Target modules:
```text
src-tauri/src/audio/core/mod.rs
src-tauri/src/audio/core/device.rs
src-tauri/src/audio/core/stream.rs
src-tauri/src/audio/core/resampler.rs
src-tauri/src/audio/core/writer.rs
```

- [ ] **Step 4: Split files manager and LLM**

Target modules:
```text
src-tauri/src/files/manager/mod.rs
src-tauri/src/files/manager/workspace.rs
src-tauri/src/files/manager/import_export.rs
src-tauri/src/files/manager/paths.rs
src-tauri/src/llm/mod.rs
src-tauri/src/llm/providers.rs
src-tauri/src/llm/client.rs
src-tauri/src/llm/prompts.rs
```

- [ ] **Step 5: Verify Rust**

Run:
```bash
cargo fmt
cargo check
cargo test --lib
```

Expected: zero Rust warnings and all library tests pass.

- [ ] **Step 6: Commit**

Run:
```bash
git add src-tauri/src/sidecar.rs src-tauri/src/sidecar src-tauri/src/local_queue.rs src-tauri/src/local_queue src-tauri/src/audio/core.rs src-tauri/src/audio/core src-tauri/src/files/manager.rs src-tauri/src/files/manager src-tauri/src/llm.rs src-tauri/src/llm
git commit -m "refactor: split Rust infrastructure modules"
```

---

### Task 10: Split Frontend Workspace Services And IPC Boundaries

**Why after backend splits:** This removes remaining component-level Tauri coupling after the command modules and large UI components are stable. Keep React as coordinator while moving IPC access behind service interfaces and workspace helper modules.

**Files:**
- Split facade: `src/lib/workspace.ts`
- Modify: `src/hooks/useLocalWorkspace.ts`
- Modify: `src/services/index.ts`
- Modify: `src/hooks/useServices.ts`
- Modify: `src/App.tsx`
- Modify: `src/components/BlockNoteEditorWithSource.tsx`
- Create: `src/lib/workspace/storage.ts`
- Create: `src/lib/workspace/notes.ts`
- Create: `src/lib/workspace/assistant.ts`
- Create: `src/lib/workspace/types.ts`
- Create: `src/services/workspace.service.ts`
- Create: `src/services/assistant-workspace.service.ts`
- Create: `src/services/settings.service.ts`
- Create: `src/services/system.service.ts`

- [ ] **Step 1: Split local workspace persistence**

Move `DEFAULT_WORKSPACE_ID`, localStorage keys, workspace normalization, active workspace selection, workspace create/update/delete, and scoped key helpers from `src/lib/workspace.ts` to `src/lib/workspace/storage.ts`. Keep `src/lib/workspace.ts` re-exporting the same public functions so existing imports continue to work.

- [ ] **Step 2: Split notes and assistant helpers**

Move note list/save/create/delete helpers to `src/lib/workspace/notes.ts`. Move assistant ask/stream/session/template/note-export helpers to `src/lib/workspace/assistant.ts`. Keep public function names and argument shapes unchanged from `src/lib/workspace.ts`.

- [ ] **Step 3: Add frontend service interfaces for remaining IPC**

Move Tauri `invoke()` calls for workspace deletion, workspace note deletion, assistant session deletion/truncation, external URL opening, and voice-input frontend event logging into service implementations:

```typescript
export interface WorkspaceService {
  deleteWorkspaceDir(workspaceFolder: string): Promise<void>;
  deleteWorkspaceNote(workspaceFolder: string, noteId: string): Promise<void>;
}

export interface AssistantWorkspaceService {
  deleteSession(sessionId: string): Promise<void>;
  deleteMessagesAfter(sessionId: string, messageId: string): Promise<void>;
}

export interface SystemService {
  openExternalUrl(url: string): Promise<void>;
  logVoiceInputFrontendEvent(event: unknown): Promise<void>;
}

export interface SettingsService {
  setLlmApiKey(apiKey: string): Promise<void>;
  deleteSummaryTemplate(id: string): Promise<void>;
  deleteAssistantPromptTemplate(id: string): Promise<void>;
}
```

- [ ] **Step 4: Replace component-level IPC**

Update `App.tsx` to use `useWorkspaceService()` for workspace directory deletion. Update `BlockNoteEditorWithSource.tsx` to use `useSystemService()` for voice-input frontend event logging. Update `RecordingSettingsTab.tsx` to use `useSettingsService()` for LLM key and template deletion operations.

- [ ] **Step 5: Verify no component-level Tauri IPC remains**

Run:
```bash
! rg -n '\binvoke\s*\(' src/components src/App.tsx
npm run build
node tests/assistant-workspace-service.test.mjs
node tests/voice-input-overlay-contract.test.mjs
```

Expected: `rg` prints no matches; build and contract tests pass.

- [ ] **Step 6: Commit**

Run:
```bash
git add src/lib/workspace.ts src/lib/workspace src/hooks/useLocalWorkspace.ts src/services src/hooks/useServices.ts src/App.tsx src/components/BlockNoteEditorWithSource.tsx tests/assistant-workspace-service.test.mjs tests/voice-input-overlay-contract.test.mjs
git commit -m "refactor: split workspace services"
```

---

### Task 11: Split Oversized Rust And Python Tests

**Files:**
- Split: `src-tauri/src/tests/db_queue.rs`
- Split: `src-tauri/src/tests/voice_input.rs`
- Split: `tests/sidecars/funasr_nano_mlx/test_pipeline.py`
- Split: `tests/sidecars/local_notes_agent/test_main.py`
- Modify: `src-tauri/src/tests/mod.rs`
- Create:
  - `src-tauri/src/tests/db_queue/mod.rs`
  - `src-tauri/src/tests/db_queue/schema.rs`
  - `src-tauri/src/tests/db_queue/jobs.rs`
  - `src-tauri/src/tests/db_queue/retry.rs`
  - `src-tauri/src/tests/db_queue/sync.rs`
  - `src-tauri/src/tests/voice_input/mod.rs`
  - `src-tauri/src/tests/voice_input/permissions.rs`
  - `src-tauri/src/tests/voice_input/text.rs`
  - `src-tauri/src/tests/voice_input/overlay.rs`
  - `tests/sidecars/funasr_nano_mlx/test_protocol.py`
  - `tests/sidecars/funasr_nano_mlx/test_transcript.py`
  - `tests/sidecars/funasr_nano_mlx/test_models.py`
  - `tests/sidecars/local_notes_agent/test_protocol.py`
  - `tests/sidecars/local_notes_agent/test_handlers.py`
  - `tests/sidecars/local_notes_agent/test_notes_mcp_server.py`

- [ ] **Step 1: Move tests by behavior**

Move schema/setup tests to `schema.rs`, enqueue/list tests to `jobs.rs`, retry/cancel tests to `retry.rs`, and synced/export tests to `sync.rs`.

- [ ] **Step 2: Keep shared fixtures private**

Place shared test DB setup helpers in `src-tauri/src/tests/db_queue/mod.rs` as `pub(super)` helpers.

- [ ] **Step 3: Split voice input tests**

Move accessibility hint and permission tests to `voice_input/permissions.rs`, text processing tests to `voice_input/text.rs`, and overlay geometry tests to `voice_input/overlay.rs`. Keep shared helpers private in `voice_input/mod.rs`.

- [ ] **Step 4: Split Python sidecar tests**

Move FunASR protocol/request tests to `test_protocol.py`, transcript formatting tests to `test_transcript.py`, and model/cache tests to `test_models.py`; keep full pipeline tests in `test_pipeline.py`. Move local notes agent protocol tests to `test_protocol.py`, command dispatch tests to `test_handlers.py`, and notes MCP tests to `test_notes_mcp_server.py`.

- [ ] **Step 5: Verify Rust and Python tests**

Run:
```bash
cargo fmt
cargo test --lib
runtime/asr/bin/python -m pytest tests/sidecars/funasr_nano_mlx
runtime/asr/bin/python -m pytest tests/sidecars/local_notes_agent
```

Expected: same Rust and Python test behaviors pass as before the split.

- [ ] **Step 6: Commit**

Run:
```bash
git add src-tauri/src/tests/db_queue.rs src-tauri/src/tests/db_queue src-tauri/src/tests/voice_input.rs src-tauri/src/tests/voice_input src-tauri/src/tests/mod.rs tests/sidecars/funasr_nano_mlx tests/sidecars/local_notes_agent
git commit -m "refactor: split oversized tests"
```

---

## Final Full Verification

Run:
```bash
cargo check
cargo test --lib
npm run build
! rg -n '\binvoke\s*\(' src/components src/App.tsx
node tests/settings-save-actions.test.mjs
node tests/hotkey-capture-ui.test.mjs
node tests/voice-input-hotkey-linkage.test.mjs
node tests/assistant-panel-ui.test.mjs
node tests/assistant-workspace-service.test.mjs
node tests/assistant-web-notice.test.mjs
node tests/voice-input-overlay-contract.test.mjs
runtime/asr/bin/python -m pytest tests/sidecars/funasr_nano_mlx
runtime/asr/bin/python -m pytest tests/sidecars/local_notes_agent
```

Expected: all commands pass.

Manual smoke after P0, P1, P2, P3, P4, P6, and P8:
```bash
npm run tauri dev
```

Check:
- App starts and stays open.
- Voice input overlay can start and stop dictation.
- Mixed recording starts, stops, and saves a file.
- ASR model status/download UI still reports progress.
- Assistant can send a normal request and a streaming request.
