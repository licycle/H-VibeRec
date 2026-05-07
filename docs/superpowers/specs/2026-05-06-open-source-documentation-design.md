# 开源 README 与 macOS 安装文档设计

## 背景

项目准备开源，需要把现有偏工程笔记的 README 调整成面向普通用户、开发者、搜索引擎和 AI 阅读器都清晰的入口文档。当前应用名为 `H-VibeRec`，核心能力是本地录音、FunASR workflow 本地转写、说话人识别、LLM 总结和本地笔记问答。当前正式用户安装路径只覆盖 macOS，安装包会通过 GitHub Releases 提供。

现有 `README.md` 已包含能力、运行、runtime 和发布验证信息，但首屏没有把非开发者的安装和首次配置路径放在前面。新的文档需要降低用户进入门槛，同时保留必要的开发者入口。

## 目标

- 生成中英文 README，采用同一个 `README.md`，中文优先，英文摘要紧随其后。
- 新增中文部署/安装文档 `docs/DEPLOYMENT.zh-CN.md`。
- 新增英文部署/安装文档 `docs/DEPLOYMENT.en.md`。
- 用户文档只从 GitHub Releases 下载 `.dmg` 安装包开始，不说明如何制作 `.dmg`。
- 用户文档只覆盖 macOS 安装、首次启动、权限授权、模型准备、LLM 配置和常见问题。
- 内容符合渐进式 AI 阅读习惯：先给机器可摘要的一句话和关键词，再给普通用户快速路径，最后给细节、故障排查和开发者信息。
- 保留开源协作需要的开发者运行命令、许可证和隐私说明。

## 非目标

- 不新增或修改应用功能。
- 不写其他平台安装说明。
- 不说明发布者如何构建、签名或制作 DMG。
- 不引入新的文档站点、静态站生成器或截图资产。
- 不改许可证文件。

## 文档信息架构

### `README.md`

`README.md` 是仓库首页和搜索入口，采用紧凑双语结构。

推荐章节顺序：

1. `# H-VibeRec`
2. 中文一句话：面向 macOS 的本地优先录音、转写和会议纪要桌面应用。
3. 英文一句话：A local-first macOS desktop app for recording, transcription, speaker-aware notes, and LLM summaries.
4. 关键词：macOS、local-first、Tauri、React、Rust、FunASR、paraformer-zh、speaker diarization、meeting notes、OpenAI-compatible Chat Completions API。
5. `当前支持 / Supported Platform`：只列 macOS。
6. `适合谁 / Who It Is For`：非开发者、会议记录者、研究者、需要本地转写和总结的人。
7. `快速开始 / Quick Start for macOS`：下载 DMG、打开 DMG、拖到 Applications、首次启动、授权权限、准备模型、配置 LLM。
8. `核心功能 / Features`：本地录音、导入音频、本地 ASR、说话人识别、LLM 总结、本地 AI 问答、语音输入法。
9. `安装与配置文档 / Installation Guides`：链接 `docs/DEPLOYMENT.zh-CN.md` 和 `docs/DEPLOYMENT.en.md`。
10. `隐私与本地数据 / Privacy and Local Data`：说明数据目录、模型目录、API Key 存系统 keychain、本地 ASR 不依赖云端。
11. `开发者运行 / Developer Setup`：保留 `npm install`、`npm run runtime:ensure`、`npm run tauri dev`。
12. `Repository Layout`：简短链接到 `docs/REPO_LAYOUT.md`。
13. `License`：AGPL-3.0-or-later。

README 不包含完整故障排查，不包含 DMG 制作命令，不包含其他平台说明。

### `docs/DEPLOYMENT.zh-CN.md`

中文文档面向非开发者，标题使用“macOS 安装与首次配置”。文档从用户已经来到 GitHub Releases 页面开始。

推荐章节顺序：

1. `# H-VibeRec macOS 安装与首次配置`
2. `你需要准备什么`：macOS、可联网下载模型、可选 LLM API Key。
3. `下载安装包`：从 GitHub Releases 下载最新 `.dmg`。
4. `打开 DMG 并安装`：双击 `.dmg`，把 `H-VibeRec.app` 拖到 `Applications`。
5. `首次启动`：从 Applications 打开；若 macOS 拦截，使用右键打开或在系统设置中允许打开。
6. `授权系统权限`：麦克风；系统音频录制；辅助功能仅在启用语音输入法时需要。
7. `准备本地转写模型`：进入设置的检测页，点击“下载/检查 FunASR workflow 模型”；解释 ModelScope 和 Hugging Face；说明代理字段用途。
8. `配置 LLM 总结`：支持 OpenAI-compatible Base URL；填写 Provider、Base URL、Model、API Key；点击测试 LLM。
9. `开始使用`：创建本地空间、录音或导入音频、转写、生成总结、使用 AI 问答。
10. `本地数据位置`：`~/Documents/Voice Vibe Local`，包含 SQLite、模型、工作空间、录音、转写和总结输出。
11. `常见问题`：DMG 打不开、应用被 macOS 拦截、模型下载慢、模型下载失败、LLM 测试失败、麦克风没有声音、语音输入法不能写入当前输入框。

中文文档使用直接行动句，避免要求用户了解 Node.js、Rust、Tauri 或源码构建。

### `docs/DEPLOYMENT.en.md`

英文文档与中文文档同结构，面向英文搜索、AI 摘要和非中文用户。

推荐标题为 `H-VibeRec macOS Installation and First-Time Setup`。

关键措辞：

- Use `GitHub Releases` and `.dmg installer`.
- Use `Applications folder`, `right-click Open`, and `Privacy & Security`.
- Describe local model preparation as `Download/check FunASR workflow models`.
- Describe LLM providers as `OpenAI-compatible Chat Completions API`.
- Avoid developer packaging instructions.
- Avoid any unsupported platform references.

## 渐进式 AI 阅读设计

文档按“摘要优先、路径其次、细节最后”的顺序组织：

- README 顶部 150 到 250 字内覆盖产品名、平台、用途、核心技术和安装方式。
- 每个长文档开头给出 `Quick Summary` 或等价中文摘要，方便 AI 抽取答案。
- 使用稳定关键词和同义表达：`H-VibeRec`、`Voice Vibe Local`、`local-first`、`macOS desktop app`、`FunASR workflow`、`paraformer-zh`、`OpenAI-compatible Chat Completions API`。
- 使用可被搜索引擎识别的清晰标题，不把重要信息藏在折叠或图片里。
- 每个步骤使用短段落和编号列表，减少前后文依赖。
- 中文和英文内容保持语义一致，但不追求逐句直译。

## 配置说明边界

用户需要理解的配置项：

- `模型下载源`：中国大陆网络通常优先尝试 ModelScope，其他网络可尝试 Hugging Face。
- `HTTP Proxy`、`HTTPS Proxy`、`SOCKS / ALL Proxy`：仅在模型或 LLM 访问受网络限制时填写。
- `LLM Provider`：填写自定义名称。
- `LLM Base URL`：支持 OpenAI-compatible 服务地址。
- `LLM Model`：按所选 LLM 服务填写模型名。
- `API Key`：保存在系统 keychain，留空保存不会修改已有 key。
- `语音输入法`：可选功能，启用后需要麦克风和 macOS 辅助功能权限。

用户不需要理解的配置项：

- `runtime/asr` 生成方式。
- Tauri resource 映射。
- Python sidecar 随安装包分发的内部细节。
- 发布者自检命令。

## 错误与故障排查设计

故障排查优先覆盖普通用户会遇到的问题：

- macOS 提示无法验证开发者：说明右键打开和系统设置允许打开。
- 无法录音：检查麦克风权限，重新打开应用。
- 无法录制系统声音：检查 macOS 隐私与安全性中的屏幕与系统音频录制。
- 模型下载慢或失败：切换模型下载源，填写代理，重新点击下载/检查。
- LLM 测试失败：确认 Base URL、Model、API Key 和网络。
- 语音输入法无法插入文本：确认辅助功能权限，避开密码框、安全输入、阻止粘贴的应用和远程桌面场景。

故障排查不讨论开发者编译、签名或发布问题。

## 验证策略

文档落地后做以下验证：

- 检查 `README.md`、`docs/DEPLOYMENT.zh-CN.md`、`docs/DEPLOYMENT.en.md` 中没有其他平台或发布制品制作流程说明。
- 检查所有相对链接能指向仓库内存在的文件。
- 检查安装步骤从 GitHub Releases `.dmg` 开始。
- 检查 README 仍保留开发者本地运行命令。
- 检查文档没有待处理标记、未完成内容或空章节。
