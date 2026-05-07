# Simple Recorder 会议空间重构计划

## 项目概述

将 Simple Recorder 从多会议管理模式重构为类似 Git 的本地/远程空间模式。

## 目标

- **本地工作区**：单一的本地会议空间（类似 git working directory）
- **远程空间**：从服务器获取的已提交会议空间列表（只读查看）
- **UI 重组**：左侧栏分为本地空间和远程空间两个区域

## MVP 范围

✅ 实现内容：
- 本地单一工作区管理
- 远程空间列表显示
- 远程文件列表查看（仅元数据）
- 本地/远程模式切换
- 网络状态检测

❌ 不包含：
- 远程文件内容预览（音频播放、文本查看）
- 提交本地工作区到远程（git push）
- 远程空间增删改查操作
- 搜索和过滤功能
- 离线数据缓存

---

## 架构设计

### 数据模型

```typescript
// 本地工作区（单一）
LocalWorkspace {
  id: 'local-workspace',
  type: 'local',
  title: string,
  created: string,
  updated: string
}

// 远程空间（服务器）
RemoteSpace {
  id: string,
  type: 'remote',
  name: string,
  description?: string,
  user_role: 'OWNER' | 'EDITOR' | 'VIEWER',
  created_by: string,
  created_at: string
}

// 远程文件
RemoteFile {
  id: string,
  name: string,
  file_type: 'AUDIO' | 'DOCUMENT',
  size: number,
  created_at: string
}
```

### UI 布局

```
┌─────────────┬─────────────────┬──────────────┐
│  左侧栏      │   中间区域       │   右侧栏      │
│             │                 │              │
│┌───────────┐│ [本地模式]       │              │
││📝 本地工作区││ EditorCenter    │ 录音列表      │
│└───────────┘│ + Vditor        │ 笔记列表      │
│─────────────│                 │ AI问答        │
│┌───────────┐│ [远程模式]       │              │
││🌐 空间1    ││ RemoteViewer    │ (隐藏)       │
││🌐 空间2    ││ 文件列表        │              │
││🌐 空间3    ││ (只读)          │              │
│└───────────┘│                 │              │
└─────────────┴─────────────────┴──────────────┘
```

### localStorage 结构

```
localStorage
├── local-workspace              → LocalWorkspace 元数据
├── local-notes                  → NoteDoc[] (本地笔记)
├── local-recordings             → string[] (本地录音ID)
├── auth-info                    → { token, user }
├── recorder-settings            → 设置信息
└── workspace-migration-completed → 迁移标记
```

---

## 实现进度

### ✅ 阶段 1: 服务层和数据层（已完成）

- [x] `src/types.ts` - 扩展类型定义（SpaceMode, LocalWorkspace, RemoteSpace, RemoteFile）
- [x] `src/lib/api.ts` - API 客户端（Bearer Token认证）
- [x] `src/services/auth.ts` - 认证服务
- [x] `src/services/remoteSpace.ts` - 远程空间服务
- [x] `src/lib/workspace.ts` - 本地工作区管理（替代原 meetings.ts）
- [x] `src/lib/migration.ts` - 数据迁移（多会议 → 单工作区）

### ✅ 阶段 2: 新 UI 组件（已完成）

- [x] `LocalWorkspaceSection.tsx` - 本地工作区区域
- [x] `RemoteSpacesSection.tsx` - 远程空间列表区域
- [x] `RemoteFileList.tsx` - 远程文件列表
- [x] `RemoteSpaceViewer.tsx` - 远程空间只读查看器
- [x] `NetworkStatusBanner.tsx` - 网络状态提示

### ✅ 阶段 3: 组件重构（已完成）

- [x] `App.tsx` - 添加空间模式切换逻辑（已完成）
  - ✅ 添加状态: spaceMode, selectedRemoteSpace, remoteSpaces, remoteFiles
  - ✅ 集成 RemoteSpaceService
  - ✅ 实现模式切换函数
  - ✅ 集成网络状态检测
  - ✅ 运行数据迁移
  - ✅ 更新所有旧的 meetings 相关代码为 workspace

- [x] `MeetingSidebar.tsx` - 拆分为本地+远程两个区域（已完成）
  - ✅ 集成 LocalWorkspaceSection
  - ✅ 集成 RemoteSpacesSection
  - ✅ 添加视觉分隔线
  - ✅ 更新接口适配新组件

- [x] `EditorCenter.tsx` - 根据模式切换显示（已完成）
  - ✅ 本地模式: 显示原有编辑器
  - ✅ 远程模式: 显示 RemoteSpaceViewer

- [x] `RightSidebar.tsx` - 远程模式下隐藏（已完成）
  - ✅ 添加条件渲染: {spaceMode === 'local' && ...}

### ✅ 阶段 4: 样式和完善（已完成）

- [x] 添加 CSS 样式（已完成）
  - ✅ 本地/远程区域分隔样式（.space-divider）
  - ✅ 选中状态高亮（.selected）
  - ✅ 角色标识样式（.role-icon, .role-label）
  - ✅ 加载和空状态样式（.loading-state, .empty-state）
  - ✅ 远程查看器样式（.remote-space-viewer）
  - ✅ 网络状态横幅样式（.network-status-banner）
  - ✅ 文件列表表格样式
  - ✅ 暗色模式适配

- [x] 网络状态处理（已完成）
  - ✅ API 错误捕获和显示
  - ✅ 离线空状态显示
  - ✅ NetworkStatusBanner 组件

---

## API 接口

### 使用的后端接口

```
GET  /spaces                    # 列出所有空间
GET  /spaces/{space_id}         # 获取空间详情
GET  /spaces/{space_id}/files   # 获取空间文件列表
```

### 认证方式

```http
Authorization: Bearer {token}
```

Token 从 `localStorage['auth-info']` 获取。

---

## 关键技术决策

1. **单一本地工作区**：简化状态管理，符合 Git 工作区概念
2. **远程完全只读**：MVP 阶段不实现任何修改操作
3. **网络断开显示空状态**：避免缓存过期和数据不一致问题
4. **仅显示文件元数据**：不实现文件内容预览
5. **数据迁移**：自动将旧的多会议数据合并到本地工作区

---

## 测试计划

### 功能测试

- [ ] 数据迁移功能测试
  - 多会议数据合并
  - 笔记和录音正确迁移
  - 迁移标记生效

- [ ] 本地工作区功能测试
  - 录音正常工作
  - 笔记编辑正常
  - 数据持久化

- [ ] 远程空间功能测试
  - 列表加载
  - 空间选择
  - 文件列表显示
  - 只读模式限制

- [ ] 网络状态测试
  - 网络断开处理
  - API 错误处理
  - 未认证状态处理

### 回归测试

- [ ] 现有录音功能不受影响
- [ ] 现有笔记编辑功能正常
- [ ] 设置功能正常
- [ ] 认证功能正常

---

## 风险和注意事项

1. **数据迁移**：需要确保旧数据安全迁移，建议用户备份
2. **向后兼容**：新版本应能正确处理旧版本的数据
3. **网络依赖**：远程功能依赖网络，需优雅降级
4. **性能**：文件列表可能很长，需考虑分页或虚拟滚动

---

## 未来功能（V2）

- 提交本地工作区到远程（创建远程空间）
- 远程文件内容预览（音频播放、文本查看）
- 远程空间搜索和过滤
- 离线模式（缓存远程数据）
- 冲突解决机制
- 批量操作

---

## 更新日志

### 2025-11-08

**🎉 项目已完成（100%）**

**已完成内容：**

- ✅ 完成服务层实现（API 客户端、认证服务、远程空间服务）
- ✅ 完成数据层重构（本地工作区、数据迁移）
- ✅ 完成所有新 UI 组件（5个组件）
- ✅ 完成 MeetingSidebar 重构
- ✅ 完成 App.tsx 重构（集成模式切换）
- ✅ 完成 EditorCenter.tsx 修改（模式切换）
- ✅ 完成 RightSidebar.tsx 修改（条件渲染）
- ✅ 完成 CSS 样式添加（新组件样式 + 暗色模式）
- ✅ 完成网络状态处理
- ✅ 完成 plan.md 文档

**交付物：**

1. ✅ 9 个新文件（服务层、数据层、组件）
2. ✅ 3 个重构文件（App.tsx, EditorCenter.tsx, MeetingSidebar.tsx）
3. ✅ 1 个扩展文件（types.ts）
4. ✅ 500+ 行新增 CSS 样式
5. ✅ plan.md 完整文档

---

## 参考资料

- 后端 API 文档: 项目内部历史 API 资料
- 前端参考实现: 项目内部历史前端资料
- 原有会议管理: `src/lib/meetings.ts`
