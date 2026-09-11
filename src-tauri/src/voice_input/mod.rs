pub mod hotkey;
pub mod insertion;
pub(crate) mod jobs;
mod overlay;
mod platform_hotkey;
mod recorder;
pub mod text;
mod warmup;

#[cfg(test)]
pub(crate) use overlay::overlay_position_for_work_area;

#[allow(unused_imports)]
pub(crate) use warmup::{
    build_dictation_warmup_request, build_warmup_status_event, schedule_dictation_warmup,
    should_schedule_dictation_warmup_after_settings_change, should_startup_dictation_warmup,
};

use std::sync::{mpsc, Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use lazy_static::lazy_static;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::commands::local::{
    auxiliary_model_paths_for_queue, ensure_dictation_model_ready_for_queue,
};
use crate::types::{
    AppSettings, VoiceInputPermissionStatus, VoiceInputStatus, VoiceInputStatusEvent,
    VoiceInputSubmission,
};

const MIN_AUDIO_SAMPLES: usize = 12_000;
const VOICE_INPUT_ASR_TIMEOUT_SECS: u64 = 180;

lazy_static! {
    static ref STATE: Mutex<VoiceInputState> = Mutex::new(VoiceInputState::default());
    static ref HOTKEY_COMMAND_TX: Mutex<Option<mpsc::Sender<HotkeyCommand>>> = Mutex::new(None);
}

static HOTKEY_WATCHER: Once = Once::new();
static STARTUP_WARMUP: Once = Once::new();
static SHORTCUT_CAPTURE_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn set_shortcut_capture_active(active: bool) -> Result<(), String> {
    let previous = SHORTCUT_CAPTURE_ACTIVE.swap(active, std::sync::atomic::Ordering::SeqCst);
    if let Err(error) = reload_hotkey_registration() {
        SHORTCUT_CAPTURE_ACTIVE.store(previous, std::sync::atomic::Ordering::SeqCst);
        let _ = reload_hotkey_registration();
        return Err(error);
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum HotkeyCommand {
    Refresh(Option<mpsc::Sender<Result<(), String>>>),
    Apply {
        enabled: bool,
        hotkey: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Triggered(Option<Result<crate::pastebox::DictationContext, String>>),
    EnterPressed,
    EscapePressed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VoiceInputPhase {
    Idle,
    Starting,
    Listening,
    Stopping,
    Cancelled,
}

impl VoiceInputPhase {
    fn as_str(&self) -> &'static str {
        match self {
            VoiceInputPhase::Idle => "idle",
            VoiceInputPhase::Starting => "starting",
            VoiceInputPhase::Listening => "listening",
            VoiceInputPhase::Stopping => "stopping",
            VoiceInputPhase::Cancelled => "cancelled",
        }
    }
}

struct VoiceInputState {
    phase: VoiceInputPhase,
    recorder: Option<recorder::ActiveShortRecorder>,
    enter_submit: Option<platform_hotkey::EnterSubmitRegistration>,
    enter_submit_available: bool,
    hotkey_label: String,
    started_at: Option<String>,
    paste_context: Option<crate::pastebox::DictationContext>,
    settings: Option<AppSettings>,
}

pub(crate) struct VoiceInputPolishOutcome {
    pub text: String,
    pub fallback: bool,
    pub error: Option<String>,
}

impl Default for VoiceInputState {
    fn default() -> Self {
        Self {
            phase: VoiceInputPhase::Idle,
            recorder: None,
            enter_submit: None,
            enter_submit_available: false,
            hotkey_label: String::new(),
            started_at: None,
            paste_context: None,
            settings: None,
        }
    }
}

pub fn init(app: AppHandle) {
    let warmup_app = app.clone();
    STARTUP_WARMUP.call_once(move || {
        warmup::schedule_startup_dictation_warmup(warmup_app);
    });
    HOTKEY_WATCHER.call_once(move || {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut sender) = HOTKEY_COMMAND_TX.lock() {
            *sender = Some(tx.clone());
        }
        let hotkey_app = app.clone();
        thread::spawn(move || run_hotkey_registration(hotkey_app, rx));
        let _ = tx.send(HotkeyCommand::Refresh(None));
    });
}

pub fn reload_hotkey_registration() -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    send_hotkey_command(HotkeyCommand::Refresh(Some(tx)))?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "Timed out while registering voice input global hotkey".to_string())?
}

pub fn apply_hotkey_registration(enabled: bool, hotkey: &str) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    send_hotkey_command(HotkeyCommand::Apply {
        enabled,
        hotkey: hotkey.to_string(),
        reply: tx,
    })?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "Timed out while registering voice input global hotkey".to_string())?
}

pub fn status() -> VoiceInputStatus {
    match STATE.lock() {
        Ok(state) => VoiceInputStatus {
            phase: state.phase.as_str().to_string(),
            message: status_message_for_state(&state),
            started_at: state.started_at.clone(),
        },
        Err(_) => VoiceInputStatus {
            phase: "failed".to_string(),
            message: "语音输入法状态不可用".to_string(),
            started_at: None,
        },
    }
}

#[cfg(all(debug_assertions, target_os = "macos"))]
pub(crate) fn audit_dictation_context() -> Option<crate::pastebox::DictationContext> {
    STATE.lock().ok()?.paste_context.clone()
}

#[cfg(all(debug_assertions, target_os = "macos"))]
pub(crate) async fn audit_start_with_capture(
    app: AppHandle,
    context: crate::pastebox::DictationContext,
) -> Result<VoiceInputStatus, String> {
    start_dictation_with_capture(app, Ok(context)).await
}

pub async fn permission_status(app: &AppHandle) -> VoiceInputPermissionStatus {
    let microphone = crate::audio::default_input_device();
    let microphone_ok = microphone.is_ok();
    let microphone_message = microphone
        .map(|device| format!("可用：{}", device.name))
        .unwrap_or_else(|error| format!("不可用：{error}"));
    let permission = crate::pastebox::accessibility_status(app).await;
    let accessibility_ok = permission.as_ref().copied().unwrap_or(false);

    VoiceInputPermissionStatus {
        platform: std::env::consts::OS.to_string(),
        microphone_ok,
        microphone_message,
        accessibility_ok,
        accessibility_message: if let Err(error) = permission {
            error
        } else if accessibility_ok {
            "Accessibility 权限已授权".to_string()
        } else if cfg!(target_os = "macos") {
            accessibility_permission_hint(cfg!(debug_assertions))
        } else {
            "语音输入法 v1 仅支持 macOS Accessibility".to_string()
        },
    }
}

pub(crate) fn accessibility_permission_hint(debug_build: bool) -> String {
    if debug_build {
        "辅助功能服务尚未授权；开发模式请点击“请求辅助功能授权”，在系统设置中允许实际运行项（可能显示 Python、Code 或 Terminal），然后重启 npm run tauri dev。无法恢复位置时内容保留在粘贴箱".to_string()
    } else {
        "辅助功能服务尚未授权；请在系统设置 > 隐私与安全性 > 辅助功能中允许 H-VibeRec，然后重启应用。无法恢复位置时内容保留在粘贴箱".to_string()
    }
}

pub async fn start_dictation(app: AppHandle) -> Result<VoiceInputStatus, String> {
    let capture = crate::pastebox::begin_dictation(&app);
    start_dictation_with_capture(app, capture).await
}

async fn start_dictation_with_capture(
    app: AppHandle,
    capture: Result<crate::pastebox::DictationContext, String>,
) -> Result<VoiceInputStatus, String> {
    let settings = crate::db::get_runtime_settings()?;
    let hotkey_label = hotkey::display_hotkey_for_status(&settings.voice_input_hotkey);
    log::info!(
        "Voice input start requested: enabled={} hotkey={} refinement_mode={}",
        settings.voice_input_enabled,
        settings.voice_input_hotkey,
        settings.voice_input_refinement_mode
    );
    if !settings.voice_input_enabled {
        log::warn!("Voice input start rejected: feature disabled");
        return Err("语音输入法未启用，请先在设置中开启".to_string());
    }
    if crate::recording::is_recording() {
        log::warn!("Voice input start rejected: meeting recording is active");
        emit_status(
            &app,
            "failed",
            "当前正在会议录音，语音输入法暂不可用",
            None,
            None,
        );
        return Err("当前正在会议录音，语音输入法暂不可用".to_string());
    }
    let paste_context = capture?;
    let started_at = Utc::now().to_rfc3339();
    {
        let mut state = STATE
            .lock()
            .map_err(|e| format!("Failed to lock voice input state: {e}"))?;
        if state.phase != VoiceInputPhase::Idle {
            return Err("当前录音正在启动或结束，请稍候".to_string());
        }
        state.phase = VoiceInputPhase::Starting;
        state.hotkey_label = hotkey_label.clone();
        state.started_at = Some(started_at.clone());
        state.paste_context = Some(paste_context.clone());
        state.settings = Some(settings);
    }
    // The capture task was launched at the entry point. Microphone readiness,
    // recording controls and ASR must not wait for AX. This overlay cannot focus.
    emit_status(&app, "starting", "麦克风启动中，请稍候", None, None);
    let recorder = match recorder::ActiveShortRecorder::start().await {
        Ok(value) => {
            log::info!("Voice input recorder started: job={}", paste_context.id());
            value
        }
        Err(error) => {
            log::error!("Voice input recorder failed to start: {error}");
            reset_to_idle();
            emit_status(&app, "failed", &error, None, None);
            return Err(error);
        }
    };
    let listening_message = format!(
        "{} · {}",
        listening_status_message(&hotkey_label, false),
        paste_context.target_hint()
    );
    {
        let mut state = STATE
            .lock()
            .map_err(|e| format!("Failed to lock voice input state: {e}"))?;
        state.phase = VoiceInputPhase::Listening;
        state.recorder = Some(recorder);
        state.enter_submit = None;
        state.enter_submit_available = false;
        state.hotkey_label = hotkey_label.clone();
        state.started_at = Some(started_at.clone());
    }
    emit_status(&app, "listening", &listening_message, None, None);

    let mut enter_submit = match platform_hotkey::register_enter_submit() {
        Ok(registration) => Some(registration),
        Err(error) => {
            log::warn!("Voice input Enter submit is unavailable: {error}");
            None
        }
    };
    if enter_submit.is_some() {
        let updated_listening_message = format!(
            "{} · {}",
            listening_status_message(&hotkey_label, true),
            paste_context.target_hint()
        );
        let should_emit_update = if let Ok(mut state) = STATE.lock() {
            if state.phase == VoiceInputPhase::Listening
                && state.started_at.as_deref() == Some(started_at.as_str())
            {
                state.enter_submit = enter_submit.take();
                state.enter_submit_available = true;
                true
            } else {
                false
            }
        } else {
            false
        };
        if should_emit_update {
            emit_status(&app, "listening", &updated_listening_message, None, None);
        }
    }
    let capture_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (_, target) = paste_context.clone().resolve().await;
        // Serialize this update with stop/cancel/start. A late result may update
        // only its own still-listening recording, never resurrect an old overlay.
        // Run on the UI thread before taking STATE: window operations may otherwise
        // wait for a main-thread shortcut callback that is also trying to read STATE.
        let ui_app = capture_app.clone();
        let _ = capture_app.run_on_main_thread(move || {
            if let Ok(state) = STATE.lock() {
                if state.phase == VoiceInputPhase::Listening
                    && state.paste_context.as_ref().map(|context| context.id())
                        == Some(paste_context.id())
                {
                    crate::pastebox::record_dictation_capture(
                        target,
                        paste_context.capture_error(),
                    );
                    emit_status(
                        &ui_app,
                        "listening",
                        &status_message_for_state(&state),
                        None,
                        None,
                    );
                }
            }
        });
    });
    Ok(status())
}

pub async fn stop_dictation(app: AppHandle) -> Result<VoiceInputSubmission, String> {
    let (recorder, enter_submit, context, settings) = {
        let mut state = STATE.lock().map_err(|e| e.to_string())?;
        if state.phase != VoiceInputPhase::Listening {
            return Err("语音输入法当前没有在听写".into());
        }
        state.phase = VoiceInputPhase::Stopping;
        state.enter_submit_available = false;
        (
            state.recorder.take(),
            state.enter_submit.take(),
            state.paste_context.clone(),
            state.settings.take(),
        )
    };
    drop(enter_submit);
    emit_status(&app, "stopping", "正在保存录音", None, None);
    // Only stop/drain the microphone and spool audio here. No AX join, model
    // preparation, network call, or insertion is allowed on this path.
    let submitted = async {
        let recorder = recorder.ok_or("语音输入法录音状态丢失")?;
        let samples = recorder.stop().await?;
        if samples.len() < MIN_AUDIO_SAMPLES {
            return Err(format!(
                "语音太短（{}ms），请至少录制 {}ms",
                audio_duration_ms(samples.len()),
                audio_duration_ms(MIN_AUDIO_SAMPLES)
            ));
        }
        let context = context.ok_or("本次录音任务信息丢失")?;
        let settings = settings.ok_or("本次录音设置丢失")?;
        jobs::save(&context, settings, samples).await
    }
    .await;
    reset_to_idle();
    match submitted {
        Ok(job) => {
            let item = crate::db::get_paste_item(&job.id)?;
            let submission = VoiceInputSubmission {
                job_id: job.id.clone(),
                seq: item.seq,
                phase: "queued".into(),
            };
            show_navigation_feedback(&app, &format!("#{} 已提交，可继续录音", item.seq));
            jobs::submit(app.clone(), job)?;
            // UI/tray refresh must not delay the stop acknowledgement.
            tauri::async_runtime::spawn(async move {
                crate::pastebox::changed(&app).await;
            });
            Ok(submission)
        }
        Err(error) => {
            show_navigation_feedback(&app, &error);
            Err(error)
        }
    }
}

pub async fn cancel_dictation(app: AppHandle) -> Result<VoiceInputStatus, String> {
    log::info!("Voice input cancel requested");
    let (recorder, enter_submit, started_at) = {
        let mut state = STATE
            .lock()
            .map_err(|e| format!("Failed to lock voice input state: {e}"))?;
        if state.phase != VoiceInputPhase::Listening {
            return Err("语音输入法当前没有在听写".to_string());
        }
        state.phase = VoiceInputPhase::Cancelled;
        state.enter_submit_available = false;
        (
            state.recorder.take(),
            state.enter_submit.take(),
            state.started_at.clone(),
        )
    };
    drop(enter_submit);

    let Some(recorder) = recorder else {
        reset_to_idle();
        return Err("语音输入法录音状态丢失".to_string());
    };

    let message = "已取消语音输入";
    emit_status(&app, "cancelled", message, None, None);
    if let Err(error) = recorder.stop().await {
        log::error!("Voice input recorder failed to stop after cancel: {error}");
        reset_to_idle();
        emit_status(&app, "failed", &error, None, None);
        return Err(error);
    }
    reset_to_idle();

    Ok(VoiceInputStatus {
        phase: "cancelled".to_string(),
        message: message.to_string(),
        started_at,
    })
}

pub async fn toggle_dictation(app: AppHandle) -> Result<VoiceInputStatus, String> {
    let phase = STATE
        .lock()
        .map_err(|e| format!("Failed to lock voice input state: {e}"))?
        .phase
        .clone();
    match phase {
        VoiceInputPhase::Listening => {
            let _ = stop_dictation(app).await?;
            Ok(status())
        }
        VoiceInputPhase::Starting | VoiceInputPhase::Stopping | VoiceInputPhase::Cancelled => {
            Ok(status())
        }
        _ => start_dictation(app).await,
    }
}

async fn process_samples(app: &AppHandle, job: &jobs::DictationJob) -> Result<String, String> {
    let (audio_path, normalized_path) = jobs::paths(&job.id)?;
    #[cfg(all(debug_assertions, target_os = "macos"))]
    let injected = jobs::audit_response(&job.id, "asr").await;
    #[cfg(not(all(debug_assertions, target_os = "macos")))]
    let injected: Option<Result<String, String>> = None;
    let raw = match injected {
        Some(result) => result?,
        None => {
            transcribe_short_audio(app, &job.settings, &job.id, &audio_path, &normalized_path)
                .await?
        }
    };
    crate::db::finish_dictation_transcription(
        &job.id,
        &raw,
        job.settings.voice_input_refinement_mode == "ai_polish",
    )?;
    // Once raw text is durable, the audio is no longer needed for recovery.
    let _ = std::fs::remove_file(audio_path);
    let _ = std::fs::remove_file(normalized_path);
    crate::pastebox::changed(app).await;
    Ok(raw)
}

async fn finish_samples(
    app: AppHandle,
    job: jobs::DictationJob,
    raw_text: String,
) -> Result<(), String> {
    let started = Instant::now();
    let polish = if job.settings.voice_input_refinement_mode == "ai_polish" {
        let _permit = jobs::POLISH_SLOTS
            .acquire()
            .await
            .map_err(|e| e.to_string())?;
        #[cfg(all(debug_assertions, target_os = "macos"))]
        let injected = jobs::audit_response(&job.id, "polish").await;
        #[cfg(not(all(debug_assertions, target_os = "macos")))]
        let injected: Option<Result<String, String>> = None;
        let result = if let Some(result) = injected {
            result
        } else {
            match crate::db::get_llm_api_key() {
                Ok(api_key) => match tokio::time::timeout(
                    Duration::from_secs(90),
                    crate::llm::polish_voice_input_text(&raw_text, &job.settings, &api_key),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err("润色超时，已使用原始转写".into()),
                },
                Err(error) => Err(error),
            }
        };
        voice_input_text_after_polish(&raw_text, result)
    } else {
        VoiceInputPolishOutcome {
            text: raw_text.clone(),
            fallback: false,
            error: None,
        }
    };
    crate::db::finish_paste_refinement(&job.id, &polish.text, polish.error.as_deref())?;
    // The capture remains specific to this UUID and cannot replace another
    // recording's global target hint. Never sample the current foreground here.
    if let Some(context) = job.context {
        let (_, target) = context.resolve().await;
        crate::db::set_dictation_target(&job.id, target.as_ref())?;
    }
    crate::pastebox::changed(&app).await;
    let item = crate::pastebox::process_ready(&app, &job.id).await?;
    log::info!("Dictation background completed: job={} seq={} delivery={} polish_fallback={} elapsed_ms={}",
        job.id, item.seq, item.delivery_status, polish.fallback, started.elapsed().as_millis());
    let _ = app.emit("voice-input-job-completed", &item);
    // Background completion only updates the outbox/notification, never the
    // recorder state or a newer recording's overlay.
    Ok(())
}

pub(crate) fn voice_input_text_after_polish(
    raw_text: &str,
    polish_result: Result<String, String>,
) -> VoiceInputPolishOutcome {
    match polish_result {
        Ok(text) if !text.trim().is_empty() => VoiceInputPolishOutcome {
            text,
            fallback: false,
            error: None,
        },
        result => VoiceInputPolishOutcome {
            text: raw_text.to_string(),
            fallback: true,
            error: Some(result.err().unwrap_or_else(|| "润色返回空内容".into())),
        },
    }
}

pub(crate) fn build_dictation_transcribe_request(
    id: &str,
    audio_path: &str,
    normalized_path: &str,
    model_path: &str,
    ffmpeg_path: &str,
    use_gpu: bool,
    punc_model_path: &str,
) -> Value {
    let mut request = crate::sidecar::transcribe_request_with_profile(
        id,
        audio_path,
        normalized_path,
        model_path,
        ffmpeg_path,
        use_gpu,
        None,
        None,
        Some(punc_model_path),
        "dictation",
        true,
    );
    if let Some(payload) = request
        .get_mut("payload")
        .and_then(|value| value.as_object_mut())
    {
        payload.insert("reuse_model".to_string(), Value::Bool(true));
    }
    request
}

async fn transcribe_short_audio(
    app: &AppHandle,
    settings: &AppSettings,
    job_id: &str,
    audio_path: &std::path::Path,
    normalized_path: &std::path::Path,
) -> Result<String, String> {
    let total_started = Instant::now();
    jobs::stage(app, job_id, "preparing_model").await?;
    log::info!(
        "Voice input ASR model preparation started: repo={} source={} configured_path={}",
        settings.asr_model_repo,
        settings.asr_model_source,
        settings.asr_model_path.as_deref().unwrap_or("unset")
    );
    let prepare_started = Instant::now();
    let model = match ensure_dictation_model_ready_for_queue(app, settings).await {
        Ok(model) => {
            log::info!(
                "Voice input ASR model preparation completed: status={} path={} elapsed_ms={}",
                model.status,
                model.path.as_deref().unwrap_or("unset"),
                prepare_started.elapsed().as_millis()
            );
            model
        }
        Err(error) => {
            log::error!(
                "Voice input ASR model preparation failed after {} ms: {}",
                prepare_started.elapsed().as_millis(),
                error
            );
            return Err(error);
        }
    };
    let model_path = model
        .path
        .clone()
        .ok_or_else(|| "ASR model is not ready".to_string())?;
    let (_auxiliary_root, _vad_model_path, _speaker_model_path, punc_model_path) =
        auxiliary_model_paths_for_queue(settings, &model_path)?;
    log::info!(
        "Voice input ASR dictation auxiliary path: punc={} exists={}",
        punc_model_path.display(),
        punc_model_path.exists()
    );
    if !punc_model_path.exists() {
        return Err(
            "ASR punctuation model is not ready; use 下载/检查 FunASR workflow first".to_string(),
        );
    }
    let runtime_started = Instant::now();
    let runtime = crate::sidecar::resolve_asr_runtime(app)?;
    log::info!(
        "Voice input ASR runtime resolved: python={} ffmpeg={} script={} elapsed_ms={}",
        runtime.python_path.display(),
        runtime.ffmpeg_path.display(),
        runtime.script_path.display(),
        runtime_started.elapsed().as_millis()
    );
    jobs::stage(app, job_id, "transcribing").await?;
    let request_id = format!("voice-input-{}", Uuid::new_v4());
    let request = build_dictation_transcribe_request(
        &request_id,
        &audio_path.to_string_lossy(),
        &normalized_path.to_string_lossy(),
        &model_path,
        &runtime.ffmpeg_path.to_string_lossy(),
        settings.use_gpu,
        &punc_model_path.to_string_lossy(),
    );
    log::info!(
        "Voice input ASR sidecar request started: id={} audio={} normalized={} model={} use_gpu={} timeout_secs={}",
        request_id,
        audio_path.display(),
        normalized_path.display(),
        model_path,
        settings.use_gpu,
        VOICE_INPUT_ASR_TIMEOUT_SECS
    );
    let sidecar_started = Instant::now();
    // Queue waiting is unbounded; the worker times only the actual request and
    // resets its process on timeout, preventing a late response crossing jobs.
    let response = crate::asr_worker::transcribe(app, request, settings).await?;
    log::info!(
        "Voice input ASR completed: job={} elapsed_ms={}",
        job_id,
        sidecar_started.elapsed().as_millis()
    );
    let sidecar_total_asr_ms = response
        .pointer("/result/timing/total_asr_ms")
        .and_then(|value| value.as_i64());
    let sidecar_infer_ms = response
        .pointer("/result/timing/asr_infer_ms")
        .and_then(|value| value.as_i64());
    let sidecar_normalize_ms = response
        .pointer("/result/timing/normalize_audio_ms")
        .and_then(|value| value.as_i64());
    let result = response
        .get("result")
        .cloned()
        .ok_or_else(|| "Sidecar response missing result".to_string())?;
    let text = result
        .get("plain_text")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| result.get("text").and_then(|value| value.as_str()))
        .map(text::plain_transcript_for_voice_input)
        .unwrap_or_default()
        .trim()
        .to_string();
    if text.is_empty() {
        log::warn!("Voice input ASR returned empty transcript: id={request_id}");
        return Err("ASR returned empty transcript".to_string());
    }
    log::info!(
        "Voice input ASR transcript parsed: id={} chars={} total_ms={} sidecar_total_asr_ms={:?} sidecar_infer_ms={:?} sidecar_normalize_ms={:?}",
        request_id,
        text::count_inserted_chars(&text),
        total_started.elapsed().as_millis(),
        sidecar_total_asr_ms,
        sidecar_infer_ms,
        sidecar_normalize_ms
    );
    Ok(text)
}

fn run_hotkey_registration(app: AppHandle, rx: mpsc::Receiver<HotkeyCommand>) {
    if let Err(error) = platform_hotkey::install_event_handler() {
        log::warn!("Failed to install voice input global hotkey handler: {error}");
    }

    let mut active_registration: Option<platform_hotkey::RegisteredHotkey> = None;
    let mut registered_signature: Option<(u32, u32)> = None;

    while let Ok(command) = rx.recv() {
        match command {
            HotkeyCommand::Refresh(reply) => {
                let refresh_result = match crate::db::get_runtime_settings() {
                    Ok(settings) => apply_hotkey_settings(
                        settings.voice_input_enabled,
                        &settings.voice_input_hotkey,
                        &mut active_registration,
                        &mut registered_signature,
                    ),
                    Err(error) => Err(error),
                };
                if let Some(reply) = reply {
                    let _ = reply.send(refresh_result);
                }
            }
            HotkeyCommand::Apply {
                enabled,
                hotkey,
                reply,
            } => {
                let result = apply_hotkey_settings(
                    enabled,
                    &hotkey,
                    &mut active_registration,
                    &mut registered_signature,
                );
                let _ = reply.send(result);
            }
            HotkeyCommand::Triggered(capture) => {
                let app_for_task = app.clone();
                tauri::async_runtime::spawn(async move {
                    let result = if let Some(capture) = capture {
                        start_dictation_with_capture(app_for_task.clone(), capture).await
                    } else if is_listening_phase() {
                        stop_dictation(app_for_task.clone()).await.map(|_| status())
                    } else {
                        Ok(status())
                    };
                    if let Err(error) = result {
                        emit_status(&app_for_task, "failed", &error, None, None);
                    }
                });
            }
            HotkeyCommand::EnterPressed => {
                if !is_listening_phase() {
                    continue;
                }
                let app_for_task = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = stop_dictation(app_for_task.clone()).await {
                        if error.contains("没有在听写") {
                            log::debug!("Voice input Enter submit ignored after phase change");
                        } else {
                            emit_status(&app_for_task, "failed", &error, None, None);
                        }
                    }
                });
            }
            HotkeyCommand::EscapePressed => {
                if !is_listening_phase() {
                    continue;
                }
                let app_for_task = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = cancel_dictation(app_for_task.clone()).await {
                        if error.contains("没有在听写") {
                            log::debug!("Voice input Esc cancel ignored after phase change");
                        } else {
                            emit_status(&app_for_task, "failed", &error, None, None);
                        }
                    }
                });
            }
        }
    }
}

fn apply_hotkey_settings(
    enabled: bool,
    hotkey_value: &str,
    active_registration: &mut Option<platform_hotkey::RegisteredHotkey>,
    registered_signature: &mut Option<(u32, u32)>,
) -> Result<(), String> {
    if !enabled || SHORTCUT_CAPTURE_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
        *active_registration = None;
        *registered_signature = None;
        log::info!("Voice input global hotkey is disabled");
        return Ok(());
    }

    let parsed = hotkey::parse_hotkey(hotkey_value)?;
    let next_signature = Some((parsed.carbon_key_code(), parsed.carbon_modifiers()));
    if next_signature == *registered_signature {
        return Ok(());
    }

    let previous_registration = active_registration.take();
    let previous_signature = *registered_signature;
    *registered_signature = None;

    match platform_hotkey::register(&parsed) {
        Ok(registration) => {
            log::info!(
                "Voice input global hotkey registered: key_code={} modifiers={}",
                parsed.carbon_key_code(),
                parsed.carbon_modifiers()
            );
            *registered_signature = next_signature;
            *active_registration = Some(registration);
            Ok(())
        }
        Err(error) => {
            *active_registration = previous_registration;
            *registered_signature = previous_signature;
            Err(error)
        }
    }
}

fn send_hotkey_command(command: HotkeyCommand) -> Result<(), String> {
    let sender = HOTKEY_COMMAND_TX
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().cloned());
    if let Some(sender) = sender {
        sender
            .send(command)
            .map_err(|_| "Voice input global hotkey worker is unavailable".to_string())
    } else {
        Err("Voice input global hotkey worker is not initialized".to_string())
    }
}

fn notify_hotkey_triggered() {
    if SHORTCUT_CAPTURE_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    // Launch capture before enqueueing recorder work. This command owns the
    // result, so scheduling delays cannot retarget it to the later foreground.
    let capture = if is_idle_phase() {
        Some(crate::pastebox::begin_dictation_from_hotkey())
    } else {
        None
    };
    let _ = send_hotkey_command(HotkeyCommand::Triggered(capture));
}

fn notify_enter_pressed() {
    let _ = send_hotkey_command(HotkeyCommand::EnterPressed);
}

fn notify_escape_pressed() {
    let _ = send_hotkey_command(HotkeyCommand::EscapePressed);
}

pub(crate) fn is_enter_key_code(key_code: i64) -> bool {
    matches!(key_code, 36 | 76)
}

pub(crate) fn is_escape_key_code(key_code: i64) -> bool {
    matches!(key_code, 53)
}

fn is_listening_phase() -> bool {
    STATE
        .lock()
        .map(|state| state.phase == VoiceInputPhase::Listening)
        .unwrap_or(false)
}

fn is_idle_phase() -> bool {
    STATE
        .lock()
        .map(|state| state.phase == VoiceInputPhase::Idle)
        .unwrap_or(false)
}

fn reset_to_idle() {
    let enter_submit = if let Ok(mut state) = STATE.lock() {
        state.phase = VoiceInputPhase::Idle;
        state.recorder = None;
        state.enter_submit_available = false;
        state.hotkey_label.clear();
        state.started_at = None;
        if let Some(context) = state.paste_context.take() {
            context.release_recording();
        }
        state.settings = None;
        state.enter_submit.take()
    } else {
        None
    };
    drop(enter_submit);
}

pub(crate) fn show_navigation_feedback(app: &AppHandle, message: &str) {
    let ui_app = app.clone();
    let message = message.to_string();
    // Check on the UI thread at publication time: a new shortcut may have
    // started another recording while this feedback was waiting to be shown.
    let _ = app.run_on_main_thread(move || {
        if let Ok(state) = STATE.lock() {
            if state.phase == VoiceInputPhase::Idle {
                emit_status(
                    &ui_app,
                    "queued",
                    &message,
                    None,
                    Some("background_feedback".into()),
                );
            }
        }
    });
}

pub(crate) fn emit_status(
    app: &AppHandle,
    phase: &str,
    message: &str,
    char_count: Option<i64>,
    insertion_strategy: Option<String>,
) {
    overlay::update_voice_input_overlay(app, phase);
    log::info!(
        "Voice input status event: phase={} message={} char_count={:?} strategy={:?}",
        phase,
        message,
        char_count,
        insertion_strategy
    );
    let _ = app.emit(
        "voice-input-status",
        VoiceInputStatusEvent {
            phase: phase.to_string(),
            message: message.to_string(),
            char_count,
            insertion_strategy,
        },
    );
    if phase == "inserted" || phase == "copied" || phase == "failed" {
        if let Ok(stats) = crate::db::get_voice_input_stats() {
            let _ = app.emit("voice-input-stats-updated", stats);
        }
    }
}

fn status_message_for_state(state: &VoiceInputState) -> String {
    match state.phase {
        VoiceInputPhase::Idle => "待命".to_string(),
        VoiceInputPhase::Starting => "麦克风启动中，请稍候".to_string(),
        VoiceInputPhase::Listening => {
            let message =
                listening_status_message(&state.hotkey_label, state.enter_submit_available);
            match &state.paste_context {
                Some(context) => format!("{message} · {}", context.target_hint()),
                None => message,
            }
        }
        VoiceInputPhase::Stopping => "正在保存录音".to_string(),
        VoiceInputPhase::Cancelled => "已取消语音输入".to_string(),
    }
}

pub(crate) fn listening_status_message(_hotkey_label: &str, enter_available: bool) -> String {
    if enter_available {
        "正在听写 · Enter 完成 · Esc 取消".to_string()
    } else {
        "正在听写".to_string()
    }
}

fn audio_duration_ms(samples: usize) -> u64 {
    (samples as u64).saturating_mul(1_000) / crate::audio::TARGET_SAMPLE_RATE as u64
}
