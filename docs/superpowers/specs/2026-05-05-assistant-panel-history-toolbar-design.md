# AI 问答右侧栏工具栏与历史编辑设计

## 背景

右侧栏 AI 问答当前使用了独立按钮样式，联网功能是普通按钮，工具栏高度会随控件变化挤压消息区。历史对话只能追加，不能删除会话，也不能从某条用户输入重新开始。

## 目标

- 采用草图 A：固定三行工具栏作为展开态，并支持折叠工具栏以增加消息区高度。
- 按钮复用现有通用控件尺寸：图标按钮使用 `mini-btn`/`toolbar-icon-btn` 的 28px 规格，表单控件保持 32px 左右高度。
- 联网功能改为 switch 样式，后续可在同一工具栏区域继续增加开关。
- 支持删除历史会话。
- 支持编辑一条用户输入：保存修改后删除该用户输入之后的消息，并从修改后的输入重新生成后续对话。
- 每个本地空间记住 AI 问答选项，应用重启后保持一致。

## UI 设计

展开态工具栏固定为三行：

1. 会话选择、创建新会话、删除当前会话。
2. 范围分段控件、Prompt 模板选择、联网 switch。
3. 轻量状态行：显示“当前空间已记住会话、范围、模板、联网状态”，并提供折叠按钮。

折叠态工具栏固定为一行：

- 显示当前会话标题的截断文本、展开按钮、联网 switch 的当前状态。
- 范围和模板继续按已保存设置生效，但不占用可见高度。

消息操作：

- 用户消息下方显示编辑和删除图标按钮。
- 编辑时把气泡替换为 textarea 与“取消 / 重新提问”按钮。
- “重新提问”会提示用户：该条之后的消息会被丢弃。
- 助手消息继续保留“保存回答为笔记”和引用来源操作。

## 持久化设计

会话和消息仍保存在 SQLite 中，由后端 Tauri 命令维护。

AI 问答选项使用前端 `localStorage` 按空间保存，键名建议为：

`assistant-panel-options:${workspaceId}`

保存字段：

- `sessionId`
- `scope`
- `promptTemplateId`
- `webEnabled`
- `toolbarCollapsed`

Tauri WebView 的 `localStorage` 会随应用重启保留，适合这类前端显示偏好和当前空间选择。若用户清空应用数据或本地存储，选项会回到默认值；这不影响 SQLite 中的历史会话和消息。

## 后端行为

新增或补齐 Tauri 命令：

- `delete_assistant_session(sessionId)`：删除会话及其消息，返回 `void`。
- `delete_assistant_messages_after(messageId, includeSelf)`：删除同会话中指定消息之后的消息；用于删除某条用户消息或截断历史。
- `update_assistant_user_message_and_truncate(messageId, content)`：仅允许更新 `role = user` 的消息，更新内容后删除该消息之后的消息，并更新会话 `updated_at`。

编辑后重新提问的数据流：

1. 前端调用 `update_assistant_user_message_and_truncate`，拿到更新后的用户消息或重新加载消息列表。
2. 前端用更新后的文本调用现有 `askLocalNotesAgentStream`，复用当前 `sessionId`、`scope`、`promptTemplateId`、`webEnabled`。
3. 生成完成后刷新会话列表和消息列表。

删除历史对话的数据流：

- 删除会话：前端确认后调用 `delete_assistant_session`，从列表移除当前会话，并切换到下一个会话或“新会话”。
- 删除用户消息：前端确认后删除该用户消息及其之后的消息，保留之前上下文。

## 测试策略

- Rust 单元测试覆盖：删除会话级联删除消息、只允许编辑用户消息、编辑后截断后续消息。
- 前端轻量测试或源码检查覆盖：选项键按 `workspaceId` 保存、联网控件使用 switch 语义、编辑后调用截断再重新提问。
- 手动验证：展开/折叠工具栏、切换空间后选项恢复、应用重启后选项保持、删除会话、编辑中途取消、编辑后重新生成。

## 非目标

- 不引入云端同步 AI 问答选项。
- 不做完整会话重命名 UI。
- 不改造 Prompt 模板管理入口。
