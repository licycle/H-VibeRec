# H-VibeRec

**语言 / Language:** [中文](#中文) | [English](#english)

**关键词 / Keywords:** macOS, local-first, desktop recorder, meeting notes, FunASR workflow, paraformer-zh, speaker diarization, Tauri, React, Rust, OpenAI-compatible Chat Completions API.

## 中文

H-VibeRec 是一款面向 macOS 的本地优先桌面应用，用于会议录音、音频导入、本地 FunASR 转写、说话人区分、LLM 会议纪要和本地笔记问答。它适合不想搭建开发环境、希望下载应用后直接开始录音、转写和总结的用户。

**当前支持：** macOS

### 快速开始

1. 打开项目的 GitHub Releases 页面，下载最新的 `.dmg` 安装包。
2. 双击打开 `.dmg`，把 `H-VibeRec.app` 拖到 `Applications` 文件夹。
3. 从 `Applications` 启动 H-VibeRec。若 macOS 阻止首次打开，请右键点击应用选择 `Open`，或到 `System Settings > Privacy & Security` 允许打开。
4. 打开应用设置，进入 `检测`，依次点击 `验证录音权限`、`检测运行环境`、`下载/检查 FunASR workflow 模型`。
5. 如需会议纪要、AI 问答或语音润色，在 `基础配置` 中填写 LLM 配置，点击 `保存配置`，再点击 `测试 LLM`。
6. 如需全局语音输入，在 `语音输入法` 中启用功能并点击 `检查权限`，按页面出现的按钮完成授权。

详细说明见：[macOS 安装与首次配置](docs/DEPLOYMENT.zh-CN.md)。

### 核心功能

- 本地录音和音频导入。
- FunASR workflow 本地转写，默认使用 `paraformer-zh`、FSMN-VAD、CAM++ 和 `ct-punc-c`。
- 说话人区分、时间戳、标点恢复和结构化转写片段。
- 基于 OpenAI-compatible Chat Completions API 的会议纪要和长文本分段总结。
- 本地笔记工作区、转写结果、总结结果和 AI 问答。
- 可选语音输入法：使用全局快捷键把短语音转成当前输入框文本。

### 隐私与本地数据

- 本地 ASR 转写在用户电脑上运行，模型权重会下载到本机。
- 普通设置、录音索引、转写任务和总结结果保存在本地 SQLite。
- 默认数据目录是 `~/Documents/Voice Vibe Local`。
- LLM API Key 保存在系统钥匙串（keychain）。
- 只有在你配置并使用 LLM 总结、AI 问答或 AI 润色时，相关文本才会发送到你配置的 LLM 服务。

### 开发者运行

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

仓库结构见 [docs/REPO_LAYOUT.md](docs/REPO_LAYOUT.md)。许可证：`AGPL-3.0-or-later`，见 [LICENSE](LICENSE)。

## English

H-VibeRec is a local-first macOS desktop app for meeting recording, audio import, local FunASR transcription, speaker-aware notes, LLM summaries, and local note Q&A. It is for users who want to install an app and start recording, transcribing, and summarizing without running developer tools.

**Supported platform:** macOS

### Quick Start

1. Open the project's GitHub Releases page and download the latest `.dmg` installer.
2. Open the `.dmg` file and drag `H-VibeRec.app` into `Applications`.
3. Launch H-VibeRec from `Applications`. If macOS blocks the first launch, right-click the app and choose `Open`, or allow it in `System Settings > Privacy & Security`.
4. Open app settings, go to `Diagnostics (检测)`, then click `验证录音权限`, `检测运行环境`, and `下载/检查 FunASR workflow 模型`.
5. For meeting summaries, AI Q&A, or AI polishing, fill in LLM settings in `Basic configuration (基础配置)`, click `保存配置`, then click `测试 LLM`.
6. For global voice input, enable it in `Voice input (语音输入法)`, click `检查权限`, and follow the buttons shown on the page.

See the full guide: [macOS Installation and First-Time Setup](docs/DEPLOYMENT.en.md).

### Features

- Local recording and audio import.
- Local FunASR workflow transcription with `paraformer-zh`, FSMN-VAD, CAM++, and `ct-punc-c` by default.
- Speaker labels, timestamps, punctuation restoration, and structured transcript segments.
- Meeting notes and long-transcript summaries through an OpenAI-compatible Chat Completions API.
- Local note workspaces, transcript output, summary output, and AI Q&A.
- Optional voice input: use a global hotkey to turn short speech into text in the current input field.

### Privacy and Local Data

- Local ASR transcription runs on your computer, with model weights downloaded to your Mac.
- Settings, recording indexes, transcription jobs, and summary results are stored in a local SQLite database.
- The default data directory is `~/Documents/Voice Vibe Local`.
- The LLM API key is stored in the system keychain.
- Text is sent to your configured LLM service only when you use LLM summaries, AI Q&A, or AI polishing.

### Developer Setup

```bash
npm install
npm run runtime:ensure
npm run tauri dev
```

`npm run tauri dev` starts the Vite UI and prepares the Tauri desktop development environment. The ASR runtime is prepared by project scripts, and the app does not fall back to system Python or system ffmpeg.

Common commands:

```bash
npm run build
npm run runtime:check
cargo test --manifest-path src-tauri/Cargo.toml
```

Repository layout: [docs/REPO_LAYOUT.md](docs/REPO_LAYOUT.md). License: `AGPL-3.0-or-later`; see [LICENSE](LICENSE).
