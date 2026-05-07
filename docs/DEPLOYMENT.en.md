# H-VibeRec macOS Installation and First-Time Setup

This guide is for non-developer users. After installation, most setup happens through buttons inside H-VibeRec settings.

## Quick Summary

- Install method: download the `.dmg` from GitHub Releases, open it, and drag `H-VibeRec.app` into `Applications`.
- First setup entry: open app settings and use `Diagnostics (检测)`, `Basic configuration (基础配置)`, and `Voice input (语音输入法)`.
- Required: in `Diagnostics (检测)`, click `验证录音权限`, `检测运行环境`, and `下载/检查 FunASR workflow 模型`.
- Optional: configure an OpenAI-compatible Chat Completions API service in `Basic configuration (基础配置)`; enable global voice input in `Voice input (语音输入法)`.

## Install the App

1. Open the project's GitHub Releases page.
2. Download the latest `.dmg` file.
3. Double-click the `.dmg`.
4. Drag `H-VibeRec.app` into `Applications`.
5. Launch H-VibeRec from `Applications`.

If macOS blocks the first launch, right-click `H-VibeRec.app` in `Applications` and choose `Open`. If it is still blocked, follow the macOS prompt or security settings prompt to allow it.

## First-Time App Setup

After opening H-VibeRec, click the settings button. Follow these pages in order.

### 1. Use the Diagnostics Page

Open `Diagnostics (检测)`, then click:

1. `验证录音权限`
2. `检测运行环境`
3. `下载/检查 FunASR workflow 模型`

`验证录音权限` checks microphone and system audio access. If something is missing, the page will show a matching button, such as `打开麦克风权限设置` or `打开系统音频权限设置`. Click the button, allow H-VibeRec in the system prompt, then return to the app and click `验证录音权限` again.

`下载/检查 FunASR workflow 模型` prepares local transcription models. Wait for the progress to finish and for the model status to show ready.

### 2. Configure Summaries and Q&A in Basic Configuration

Local recording and local transcription do not require an API key. These features need an LLM:

- Meeting summaries
- Long transcript summaries
- Local note Q&A
- AI polishing mode for voice input

If you need these features, open `Basic configuration (基础配置)`:

1. Enter your OpenAI-compatible Chat Completions API service settings: `LLM Provider`, `LLM Base URL`, and `LLM Model`.
2. Paste your key into `API Key`.
3. Click `保存配置`.
4. Click `测试 LLM`.

The API key is stored in the system keychain. Saving with an empty `API Key` field does not overwrite an existing key.

If model download fails, return to `Basic configuration (基础配置)`, switch `模型下载源`, or fill in proxy fields for your network. Save settings, then return to `Diagnostics (检测)` and click `下载/检查 FunASR workflow 模型` again.

### 3. Optional: Enable Voice Input

Only configure this page if you want to use a global hotkey to insert spoken text into the current input field.

Open `Voice input (语音输入法)`:

1. Turn on `启用语音输入法`.
2. Keep or change the global hotkey.
3. Click `保存配置`.
4. Click `检查权限`.

If permission is missing, the page will show buttons such as `请求辅助功能授权`, `打开辅助功能权限设置`, or `打开麦克风权限设置`. Follow the buttons shown on the page.

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

### No audio is recorded

Open settings, go to `Diagnostics (检测)`, and click `验证录音权限`. If the page shows a permission button, click it and allow H-VibeRec in the system prompt.

### System audio is not recorded

Open settings, go to `Diagnostics (检测)`, and click `验证录音权限`. If the page shows `打开系统音频权限设置`, click it, grant permission, and restart the app.

### Model download is slow or fails

Go to `Basic configuration (基础配置)`, switch `模型下载源`, or fill in proxy fields and click `保存配置`. Then return to `Diagnostics (检测)` and click `下载/检查 FunASR workflow 模型` again.

### LLM test fails

Go to `Basic configuration (基础配置)`, check `LLM Base URL`, `LLM Model`, and `API Key`, save, then click `测试 LLM` again.

### Voice input cannot insert text

Go to `Voice input (语音输入法)`, click `检查权限`, and follow the buttons shown on the page. Password fields, secure input, apps that block paste, remote desktops, virtual machines, and some game windows may not accept inserted text.
