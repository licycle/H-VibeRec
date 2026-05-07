# Assistant Panel History Toolbar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Update the right-sidebar AI panel so its controls match the app, the toolbar can collapse, workspace options persist, and users can delete or edit history.

**Architecture:** SQLite remains the source of truth for sessions and messages. The React panel uses `src/lib/workspace.ts` service wrappers for Tauri commands, while display preferences are stored per workspace in `localStorage`.

**Tech Stack:** React + TypeScript, Tauri v2 IPC, Rust + rusqlite, Node source-check tests, Rust unit tests.

---

### Task 1: Backend History Mutation

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/commands/assistant.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/tests/db_queue.rs`

- [ ] **Step 1: Write failing Rust tests**

Add tests that create a session with `user -> assistant -> user -> assistant`, then assert:

- `delete_assistant_session` removes the session and all messages.
- `update_assistant_user_message_and_truncate` updates only a user message and removes later messages.
- Editing an assistant message is rejected.
- `recent_assistant_final_messages_before` returns only messages before the edited user message.

- [ ] **Step 2: Run the focused Rust test and verify failure**

Run: `cd src-tauri && cargo test assistant_history -- --nocapture`

Expected: compile failure because the new DB functions do not exist yet.

- [ ] **Step 3: Implement DB functions**

Add these public functions in `src-tauri/src/db.rs`:

- `delete_assistant_session(session_id: &str) -> Result<(), String>`
- `delete_assistant_messages_after(message_id: &str, include_self: bool) -> Result<(), String>`
- `update_assistant_user_message_and_truncate(message_id: &str, content: &str) -> Result<AssistantMessage, String>`
- `recent_assistant_final_messages_before(session_id: &str, message_id: &str, limit: usize) -> Result<Vec<AssistantMessage>, String>`

Use SQLite `rowid` to define message order inside a session.

- [ ] **Step 4: Expose Tauri commands**

Add commands:

- `delete_assistant_session(sessionId: String)`
- `delete_assistant_messages_after(messageId: String, includeSelf: bool)`
- `update_assistant_user_message_and_truncate(messageId: String, content: String)`

Register them in `commands/mod.rs` and `lib.rs`.

- [ ] **Step 5: Support edited-message regeneration**

Add optional `reuseUserMessageId` to `AskLocalNotesAgentRequest`. When present, validate that message belongs to the session and has role `user`, build history with `recent_assistant_final_messages_before`, and insert only the assistant reply.

- [ ] **Step 6: Run backend tests**

Run: `cd src-tauri && cargo test assistant_history -- --nocapture`

Expected: tests pass.

### Task 2: Frontend Service Surface

**Files:**
- Modify: `src/types.ts`
- Modify: `src/lib/workspace.ts`
- Test: `tests/assistant-workspace-service.test.mjs`

- [ ] **Step 1: Write failing Node source-check assertions**

Assert that `workspace.ts` exports wrappers for the three history commands and passes `reuseUserMessageId` through `ask_local_notes_agent_stream`.

- [ ] **Step 2: Run test and verify failure**

Run: `node tests/assistant-workspace-service.test.mjs`

Expected: assertion failure for missing wrappers.

- [ ] **Step 3: Implement service wrappers**

Add:

- `deleteAssistantSession(sessionId: string)`
- `deleteAssistantMessagesAfter(messageId: string, includeSelf: boolean)`
- `updateAssistantUserMessageAndTruncate(messageId: string, content: string)`

Extend `askLocalNotesAgentStream` params with `reuseUserMessageId?: string | null`.

- [ ] **Step 4: Run service test**

Run: `node tests/assistant-workspace-service.test.mjs`

Expected: pass.

### Task 3: Assistant Panel UI And Persistence

**Files:**
- Modify: `src/components/AssistantPanel.tsx`
- Modify: `src/components/AssistantPanel.css`
- Test: `tests/assistant-panel-ui.test.mjs`

- [ ] **Step 1: Write failing panel source-check test**

Assert the panel includes:

- `ASSISTANT_PANEL_OPTIONS_PREFIX`
- `toolbarCollapsed`
- `role="switch"`
- `deleteAssistantSession`
- `updateAssistantUserMessageAndTruncate`
- `reuseUserMessageId`

- [ ] **Step 2: Run test and verify failure**

Run: `node tests/assistant-panel-ui.test.mjs`

Expected: assertion failure for missing UI/persistence code.

- [ ] **Step 3: Implement per-workspace options**

Load and save `sessionId`, `scope`, `promptTemplateId`, `webEnabled`, and `toolbarCollapsed` using key `assistant-panel-options:${workspaceId}`.

- [ ] **Step 4: Implement toolbar**

Use fixed expanded toolbar rows, collapsed toolbar row, common 28px icon button classes, and switch styling for联网.

- [ ] **Step 5: Implement history actions**

Add session delete, user-message delete, and user-message edit/regenerate flows. Use `confirmLocalAction` for destructive or truncating operations.

- [ ] **Step 6: Run panel source-check test**

Run: `node tests/assistant-panel-ui.test.mjs`

Expected: pass.

### Task 4: Final Verification

**Files:**
- Verify project-wide TypeScript and frontend build.
- Verify focused Rust tests.

- [ ] **Step 1: Run JS source-check tests**

Run:

```bash
node tests/assistant-workspace-service.test.mjs
node tests/assistant-panel-ui.test.mjs
```

Expected: both pass.

- [ ] **Step 2: Run focused Rust tests**

Run: `cd src-tauri && cargo test assistant_history -- --nocapture`

Expected: pass.

- [ ] **Step 3: Run build**

Run: `npm run build`

Expected: TypeScript and Vite build pass.
