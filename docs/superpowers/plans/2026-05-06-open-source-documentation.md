# Open Source Documentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a bilingual open-source README and macOS-only end-user installation documents for H-VibeRec.

**Architecture:** Keep `README.md` as the repository entry point with progressive bilingual content, then move detailed non-developer installation and first-time setup instructions into language-specific docs. Do not document DMG creation or unsupported platform installation paths.

**Tech Stack:** Markdown documentation for a Tauri v2, React, TypeScript, Rust, FunASR workflow, and OpenAI-compatible Chat Completions API desktop app.

---

## File Structure

- Modify: `README.md`
  - Responsibility: public repository landing page, bilingual summary, macOS quick start, feature overview, privacy note, developer setup, and documentation links.
- Create: `docs/DEPLOYMENT.zh-CN.md`
  - Responsibility: Chinese macOS installation and first-time setup guide for non-developer users who download a DMG from GitHub Releases.
- Create: `docs/DEPLOYMENT.en.md`
  - Responsibility: English macOS installation and first-time setup guide with equivalent meaning for search engines, AI readers, and non-Chinese users.

## Task 1: Replace README With Bilingual Open-Source Entry

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Replace README content**

Use `apply_patch` to replace the existing internal README with a bilingual open-source entry. The new README must include these sections in this order:

```markdown
# H-VibeRec

H-VibeRec 是一款面向 macOS 的本地优先桌面应用，用于会议录音、音频导入、本地 FunASR 转写、说话人区分、LLM 会议纪要和本地笔记问答。

H-VibeRec is a local-first macOS desktop app for meeting recording, audio import, local FunASR transcription, speaker-aware notes, LLM summaries, and local note Q&A.

**Keywords:** macOS, local-first, desktop recorder, meeting notes, FunASR workflow, paraformer-zh, speaker diarization, Tauri, React, Rust, OpenAI-compatible Chat Completions API.

## 当前支持 / Supported Platform

- macOS

## 适合谁 / Who It Is For

- 不想自己搭建开发环境，只想下载应用并开始录音、转写和总结的用户。
- 需要把会议、访谈、课堂、播客或研究音频整理成本地笔记的人。
- 希望录音和转写数据优先保存在本机，同时按需使用 LLM 生成总结的人。
- Users who want to install a desktop app, record or import audio, transcribe locally, and generate meeting notes without running developer tools.

## 快速开始 / Quick Start for macOS

1. 打开项目的 GitHub Releases 页面，下载最新的 `.dmg` 安装包。
2. 双击打开 `.dmg`，把 `H-VibeRec.app` 拖到 `Applications` 文件夹。
3. 从 `Applications` 启动 H-VibeRec。若 macOS 阻止首次打开，请右键点击应用选择 `Open`，或到 `System Settings > Privacy & Security` 允许打开。
4. 按系统提示允许麦克风权限。若要录制系统声音或使用语音输入法，请在系统隐私设置中授权对应权限。
5. 打开应用设置，进入检测页，点击 `下载/检查 FunASR workflow 模型` 准备本地转写模型。
6. 如需会议纪要、AI 问答或语音润色，在基础配置中填写 OpenAI-compatible Chat Completions API 的 Provider、Base URL、Model 和 API Key，然后点击 `测试 LLM`。

Detailed guides:

- [中文：macOS 安装与首次配置](docs/DEPLOYMENT.zh-CN.md)
- [English: macOS Installation and First-Time Setup](docs/DEPLOYMENT.en.md)

## 核心功能 / Features

- 本地录音和音频导入。
- FunASR workflow 本地转写，默认使用 `paraformer-zh`、FSMN-VAD、CAM++ 和 `ct-punc-c`。
- 说话人区分、时间戳、标点恢复和结构化转写片段。
- 基于 OpenAI-compatible Chat Completions API 的会议纪要和长文本分段总结。
- 本地笔记工作区、转写结果、总结结果和 AI 问答。
- 可选语音输入法：使用全局快捷键把短语音转成当前输入框文本。

## 隐私与本地数据 / Privacy and Local Data

- 本地 ASR 转写在用户电脑上运行，模型权重下载安装到本机。
- 普通设置、录音索引、转写任务和总结结果保存在本地 SQLite。
- 默认数据目录是 `~/Documents/Voice Vibe Local`。
- LLM API Key 保存在系统 keychain。
- 只有在你配置并使用 LLM 总结、AI 问答或 AI 润色时，相关文本才会发送到你配置的 LLM 服务。

## 用户文档 / User Documentation

- [中文安装与配置](docs/DEPLOYMENT.zh-CN.md)
- [English installation and setup](docs/DEPLOYMENT.en.md)

## 开发者运行 / Developer Setup

```bash
npm install
npm run runtime:ensure
npm run tauri dev
```

`npm run tauri dev` 会启动 Vite UI，并准备 Tauri 桌面开发环境。ASR runtime 固定由项目脚本准备，应用不会回退到系统 Python 或系统 ffmpeg。

常用命令：

```bash
npm run build
npm run runtime:check
cargo test --manifest-path src-tauri/Cargo.toml
```

## Repository Layout

See [docs/REPO_LAYOUT.md](docs/REPO_LAYOUT.md).

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
```

- [ ] **Step 2: Check README scope**

Run:

```bash
rg -n "Windows|Linux|package-app|package:check|tauri build|bundle:verify-runtime|how to build a DMG|打包|制作 DMG" README.md
```

Expected: no matches, except if a match appears only in a phrase explicitly saying the README does not contain release-building instructions. Prefer no matches at all.

## Task 2: Add Chinese macOS Installation Guide

**Files:**
- Create: `docs/DEPLOYMENT.zh-CN.md`

- [ ] **Step 1: Create Chinese deployment guide**

Use `apply_patch` to create `docs/DEPLOYMENT.zh-CN.md` with these sections:

```markdown
# H-VibeRec macOS 安装与首次配置

这份文档面向非开发者：你只需要从 GitHub Releases 下载 `.dmg` 安装包，不需要安装 Node.js、Rust、Tauri 或 Python。

## 快速摘要

- 当前用户安装路径：macOS。
- 安装方式：下载 `.dmg`，打开后把 `H-VibeRec.app` 拖到 `Applications`。
- 首次使用需要：麦克风权限、本地 FunASR workflow 模型、可选 LLM API Key。
- 本地数据默认保存在 `~/Documents/Voice Vibe Local`。

## 你需要准备什么

1. 一台 macOS 电脑。
2. 可联网环境，用于首次下载本地转写模型。
3. 可选：OpenAI-compatible Chat Completions API 服务的 API Key，用于会议纪要、AI 问答和 AI 润色。

## 下载安装包

1. 打开项目的 GitHub Releases 页面。
2. 找到最新版本。
3. 下载名称以 `.dmg` 结尾的安装包。
4. 下载完成后，在 Finder 的 Downloads 文件夹中找到该文件。

## 打开 DMG 并安装

1. 双击 `.dmg` 文件。
2. 在打开的窗口中，把 `H-VibeRec.app` 拖到 `Applications` 文件夹。
3. 等待复制完成。
4. 可以关闭 DMG 窗口，并在 Finder 侧边栏弹出安装盘。

## 首次启动

1. 打开 Finder。
2. 进入 `Applications` 文件夹。
3. 双击 `H-VibeRec.app`。

如果 macOS 提示无法验证开发者或阻止打开：

1. 在 `Applications` 中右键点击 `H-VibeRec.app`。
2. 选择 `Open`。
3. 在确认窗口中再次选择 `Open`。

如果仍无法打开：

1. 打开 `System Settings`。
2. 进入 `Privacy & Security`。
3. 在安全提示区域允许打开 H-VibeRec。
4. 回到 `Applications` 再次启动应用。

## 授权系统权限

### 麦克风

录音和语音输入需要麦克风权限。首次录音时，macOS 会弹出权限提示，请选择允许。

如果之前拒绝了权限：

1. 打开 `System Settings`。
2. 进入 `Privacy & Security > Microphone`。
3. 找到 H-VibeRec 并打开权限。
4. 重新启动 H-VibeRec。

### 系统音频录制

如果你需要录制会议软件、浏览器或系统播放的声音，请在 macOS 隐私设置中允许屏幕与系统音频录制权限。

路径通常是：

`System Settings > Privacy & Security > Screen & System Audio Recording`

授权后建议重新启动 H-VibeRec。

### 辅助功能

只有启用语音输入法时才需要辅助功能权限。该权限用于把语音转写结果写入当前输入框。

路径通常是：

`System Settings > Privacy & Security > Accessibility`

## 准备本地转写模型

H-VibeRec 的本地转写使用 FunASR workflow。安装包包含运行环境，但模型权重需要首次使用时下载到本机。

1. 打开 H-VibeRec。
2. 点击设置按钮。
3. 进入 `检测` 页面。
4. 点击 `下载/检查 FunASR workflow 模型`。
5. 等待下载完成，直到模型状态显示已就绪。

模型下载源：

- `ModelScope 魔塔`：通常适合中国大陆网络。
- `Hugging Face`：适合可以稳定访问 Hugging Face 的网络。

如果下载速度慢或失败，可以在基础配置中填写代理：

- `HTTP Proxy`
- `HTTPS Proxy`
- `SOCKS / ALL Proxy`

填写代理后保存配置，再回到检测页重新点击 `下载/检查 FunASR workflow 模型`。

## 配置 LLM 总结

本地录音和本地转写不需要 LLM API Key。以下功能需要 LLM：

- 会议纪要
- 长文本总结
- 本地笔记 AI 问答
- 语音输入法的 AI 润色模式

示例配置：

- `LLM Provider`: `test-provider`
- `LLM Base URL`: `https://api.example.com/v1`
- `LLM Model`: `test-model`

配置步骤：

1. 打开设置。
2. 进入 `基础配置`。
3. 填写 `LLM Provider`、`LLM Base URL`、`LLM Model`。
4. 在 `API Key` 中粘贴你的 API Key。
5. 点击 `保存配置`。
6. 点击 `测试 LLM`。

API Key 会保存在系统 keychain。`API Key` 输入框留空保存时，不会修改已经保存的 key。

## 开始使用

1. 创建或选择一个本地空间。
2. 点击录音按钮开始录音，或导入已有音频。
3. 对音频执行本地转写。
4. 查看带时间戳和说话人标记的转写文本。
5. 选择总结模板生成会议纪要。
6. 在右侧 AI 问答面板中基于本地笔记提问。

## 本地数据位置

默认数据目录：

`~/Documents/Voice Vibe Local`

常见内容：

- `app.sqlite`：本地数据库。
- `models/`：本地 ASR 模型。
- `workspaces/`：本地工作空间。
- `transcripts/`：转写输出。
- `summaries/`：总结输出。

## 常见问题

### DMG 打不开

确认下载完整。如果文件来自浏览器下载列表，可以在 Finder 中重新定位该文件后再双击打开。

### macOS 阻止打开应用

在 `Applications` 中右键点击 `H-VibeRec.app`，选择 `Open`。如果仍被阻止，到 `System Settings > Privacy & Security` 允许打开。

### 模型下载慢或失败

切换 `模型下载源`，或填写代理后重新点击 `下载/检查 FunASR workflow 模型`。

### LLM 测试失败

检查 `LLM Base URL`、`LLM Model`、`API Key` 和网络。确认你的服务兼容 OpenAI Chat Completions API。

### 没有录到声音

检查麦克风权限，确认系统输入设备可用，然后重新启动 H-VibeRec。

### 无法录制系统声音

检查 `Screen & System Audio Recording` 权限。授权后重新启动应用。

### 语音输入法无法写入当前输入框

检查 `Accessibility` 权限。密码框、安全输入、阻止粘贴的应用、远程桌面、虚拟机和部分游戏窗口可能无法写入。
```

- [ ] **Step 2: Check Chinese guide scope**

Run:

```bash
rg -n "Node.js|Rust|Tauri|Python|package-app|tauri build|bundle:verify-runtime|Windows|Linux|打包|制作 DMG" docs/DEPLOYMENT.zh-CN.md
```

Expected: matches are allowed only in the opening sentence that says users do not need Node.js, Rust, Tauri, or Python; no unsupported platform or release-building instructions should appear.

## Task 3: Add English macOS Installation Guide

**Files:**
- Create: `docs/DEPLOYMENT.en.md`

- [ ] **Step 1: Create English deployment guide**

Use `apply_patch` to create `docs/DEPLOYMENT.en.md` with these sections:

```markdown
# H-VibeRec macOS Installation and First-Time Setup

This guide is for non-developer users. You only need to download the `.dmg` installer from GitHub Releases. You do not need Node.js, Rust, Tauri, or Python.

## Quick Summary

- Current end-user installation path: macOS.
- Install method: download the `.dmg`, open it, and drag `H-VibeRec.app` into `Applications`.
- First-time setup needs microphone permission, local FunASR workflow models, and an optional LLM API key.
- Local data is stored in `~/Documents/Voice Vibe Local` by default.

## What You Need

1. A Mac.
2. Network access for the first local transcription model download.
3. Optional: an OpenAI-compatible Chat Completions API key for meeting summaries, local note Q&A, and AI polishing.

## Download the Installer

1. Open the project's GitHub Releases page.
2. Find the latest release.
3. Download the file ending in `.dmg`.
4. After the download finishes, find the file in Finder, usually in `Downloads`.

## Open the DMG and Install

1. Double-click the `.dmg` file.
2. In the window that opens, drag `H-VibeRec.app` into the `Applications` folder.
3. Wait for the copy to finish.
4. Close the DMG window and eject the mounted installer disk from Finder if needed.

## First Launch

1. Open Finder.
2. Go to `Applications`.
3. Double-click `H-VibeRec.app`.

If macOS says it cannot verify the developer or blocks the app:

1. Right-click `H-VibeRec.app` in `Applications`.
2. Choose `Open`.
3. Choose `Open` again in the confirmation dialog.

If the app is still blocked:

1. Open `System Settings`.
2. Go to `Privacy & Security`.
3. Allow H-VibeRec in the security prompt area.
4. Launch the app again from `Applications`.

## Grant System Permissions

### Microphone

Recording and voice input need microphone permission. When macOS asks for permission, choose allow.

If you previously denied permission:

1. Open `System Settings`.
2. Go to `Privacy & Security > Microphone`.
3. Enable H-VibeRec.
4. Restart H-VibeRec.

### System Audio Recording

If you want to record sound from meeting apps, browsers, or other system playback, allow screen and system audio recording in macOS privacy settings.

The path is usually:

`System Settings > Privacy & Security > Screen & System Audio Recording`

Restart H-VibeRec after granting the permission.

### Accessibility

Accessibility permission is only needed when you enable the voice input feature. It lets H-VibeRec insert transcribed text into the currently focused text field.

The path is usually:

`System Settings > Privacy & Security > Accessibility`

## Prepare Local Transcription Models

H-VibeRec uses the FunASR workflow for local transcription. The installer includes the runtime, but model weights are downloaded to your Mac during first-time setup.

1. Open H-VibeRec.
2. Open settings.
3. Go to the `Diagnostics` page.
4. Click `Download/check FunASR workflow models`.
5. Wait until the model status shows ready.

Model sources:

- `ModelScope`: often works better on mainland China networks.
- `Hugging Face`: works well when Hugging Face is reliably accessible.

If the download is slow or fails, fill in a proxy in the basic settings:

- `HTTP Proxy`
- `HTTPS Proxy`
- `SOCKS / ALL Proxy`

Save settings, then return to Diagnostics and click `Download/check FunASR workflow models` again.

## Configure LLM Summaries

Local recording and local transcription do not require an LLM API key. These features need an LLM:

- Meeting summaries
- Long transcript summaries
- Local note Q&A
- AI polishing mode for voice input

Example configuration:

- `LLM Provider`: `test-provider`
- `LLM Base URL`: `https://api.example.com/v1`
- `LLM Model`: `test-model`

Setup steps:

1. Open settings.
2. Go to `Basic configuration`.
3. Fill in `LLM Provider`, `LLM Base URL`, and `LLM Model`.
4. Paste your API key into `API Key`.
5. Click `Save configuration`.
6. Click `Test LLM`.

The API key is stored in the system keychain. Saving with an empty `API Key` field does not overwrite an existing key.

## Start Using H-VibeRec

1. Create or select a local workspace.
2. Start a recording, or import an existing audio file.
3. Run local transcription on the audio.
4. Review timestamped transcript text with speaker labels.
5. Choose a summary template to generate meeting notes.
6. Ask questions about local notes in the right-side AI Q&A panel.

## Local Data Location

Default data directory:

`~/Documents/Voice Vibe Local`

Common contents:

- `app.sqlite`: local database.
- `models/`: local ASR models.
- `workspaces/`: local workspaces.
- `transcripts/`: transcript output.
- `summaries/`: summary output.

## Troubleshooting

### The DMG will not open

Check that the download completed. If the file came from your browser downloads list, reveal it in Finder and double-click it there.

### macOS blocks the app

Right-click `H-VibeRec.app` in `Applications` and choose `Open`. If it is still blocked, go to `System Settings > Privacy & Security` and allow it.

### Model download is slow or fails

Switch the `Model source`, or configure a proxy and click `Download/check FunASR workflow models` again.

### LLM test fails

Check `LLM Base URL`, `LLM Model`, `API Key`, and network access. Confirm that your provider supports the OpenAI Chat Completions API.

### No audio is recorded

Check microphone permission, confirm that the system input device works, and restart H-VibeRec.

### System audio is not recorded

Check `Screen & System Audio Recording` permission. Restart the app after granting it.

### Voice input cannot insert text

Check `Accessibility` permission. Password fields, secure input, apps that block paste, remote desktops, virtual machines, and some game windows may not accept inserted text.
```

- [ ] **Step 2: Check English guide scope**

Run:

```bash
rg -n "package-app|tauri build|bundle:verify-runtime|Windows|Linux|build a DMG|signing|notarization" docs/DEPLOYMENT.en.md
```

Expected: no matches.

## Task 4: Verify Documentation Links and Progressive Reading Constraints

**Files:**
- Verify: `README.md`
- Verify: `docs/DEPLOYMENT.zh-CN.md`
- Verify: `docs/DEPLOYMENT.en.md`

- [ ] **Step 1: Verify docs exist**

Run:

```bash
test -f README.md && test -f docs/DEPLOYMENT.zh-CN.md && test -f docs/DEPLOYMENT.en.md && test -f docs/REPO_LAYOUT.md && test -f LICENSE
```

Expected: exit code 0.

- [ ] **Step 2: Verify links are present**

Run:

```bash
rg -n "docs/DEPLOYMENT.zh-CN.md|docs/DEPLOYMENT.en.md|docs/REPO_LAYOUT.md|LICENSE" README.md
```

Expected: all four paths appear.

- [ ] **Step 3: Verify no unfinished markers**

Run:

```bash
rg -n "TO[D]O|TB[D]|待定|占位|coming soon|later" README.md docs/DEPLOYMENT.zh-CN.md docs/DEPLOYMENT.en.md
```

Expected: no matches.

- [ ] **Step 4: Verify macOS-only user-installation scope**

Run:

```bash
rg -n "Windows|Linux|package-app|package:check|tauri build|bundle:verify-runtime|npm run tauri build|制作 DMG|build a DMG|notarization|signing" README.md docs/DEPLOYMENT.zh-CN.md docs/DEPLOYMENT.en.md
```

Expected: no matches.

- [ ] **Step 5: Commit documentation changes**

Run:

```bash
git add README.md docs/DEPLOYMENT.zh-CN.md docs/DEPLOYMENT.en.md docs/superpowers/plans/2026-05-06-open-source-documentation.md
git commit -m "docs: add open source macos onboarding"
```

Expected: commit succeeds.
