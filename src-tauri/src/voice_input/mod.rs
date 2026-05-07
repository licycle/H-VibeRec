pub mod hotkey;
pub mod insertion;
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
    AppSettings, VoiceInputDictationResult, VoiceInputPermissionStatus, VoiceInputStatus,
    VoiceInputStatusEvent,
};

const MIN_AUDIO_SAMPLES: usize = 12_000;
const VOICE_INPUT_ASR_TIMEOUT_SECS: u64 = 180;

lazy_static! {
    static ref STATE: Mutex<VoiceInputState> = Mutex::new(VoiceInputState::default());
    static ref HOTKEY_COMMAND_TX: Mutex<Option<mpsc::Sender<HotkeyCommand>>> = Mutex::new(None);
}

static HOTKEY_WATCHER: Once = Once::new();
static STARTUP_WARMUP: Once = Once::new();

#[derive(Debug, Clone)]
enum HotkeyCommand {
    Refresh(Option<mpsc::Sender<Result<(), String>>>),
    Apply {
        enabled: bool,
        hotkey: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Triggered,
    EnterPressed,
    EscapePressed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VoiceInputPhase {
    Idle,
    Starting,
    Listening,
    PreparingModel,
    Transcribing,
    Refining,
    Inserting,
    Cancelled,
}

impl VoiceInputPhase {
    fn as_str(&self) -> &'static str {
        match self {
            VoiceInputPhase::Idle => "idle",
            VoiceInputPhase::Starting => "starting",
            VoiceInputPhase::Listening => "listening",
            VoiceInputPhase::PreparingModel => "preparing_model",
            VoiceInputPhase::Transcribing => "transcribing",
            VoiceInputPhase::Refining => "refining",
            VoiceInputPhase::Inserting => "inserting",
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

pub fn permission_status() -> VoiceInputPermissionStatus {
    let microphone = crate::audio::default_input_device();
    let microphone_ok = microphone.is_ok();
    let microphone_message = microphone
        .map(|device| format!("可用：{}", device.name))
        .unwrap_or_else(|error| format!("不可用：{error}"));
    let accessibility_ok = accessibility_trusted();

    VoiceInputPermissionStatus {
        platform: std::env::consts::OS.to_string(),
        microphone_ok,
        microphone_message,
        accessibility_ok,
        accessibility_message: if accessibility_ok {
            "Accessibility 权限已授权".to_string()
        } else if cfg!(target_os = "macos") {
            accessibility_permission_hint(cfg!(debug_assertions))
        } else {
            "语音输入法 v1 仅支持 macOS Accessibility".to_string()
        },
    }
}

pub fn request_accessibility_permission() -> VoiceInputPermissionStatus {
    let _ = request_accessibility_trust_prompt();
    permission_status()
}

pub(crate) fn accessibility_permission_hint(debug_build: bool) -> String {
    if debug_build {
        "未授权 Accessibility；开发模式请点击“请求辅助功能授权”，在系统设置中允许当前运行项（通常是 target/debug/hit-vvc；如果 macOS 显示 Code 或 Terminal，则允许该项），然后重启 npm run tauri dev。无法自动粘贴时会保留到剪贴板".to_string()
    } else {
        "未授权 Accessibility；请在系统设置 > 隐私与安全性 > 辅助功能中允许 H-VibeRec，然后重启应用。无法自动粘贴时会保留到剪贴板".to_string()
    }
}

pub async fn start_dictation(app: AppHandle) -> Result<VoiceInputStatus, String> {
    let settings = crate::db::get_settings()?;
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
    let started_at = Utc::now().to_rfc3339();
    {
        let mut state = STATE
            .lock()
            .map_err(|e| format!("Failed to lock voice input state: {e}"))?;
        if state.phase != VoiceInputPhase::Idle {
            return Err("语音输入法正在处理上一段输入".to_string());
        }
        state.phase = VoiceInputPhase::Starting;
        state.hotkey_label = hotkey_label.clone();
        state.started_at = Some(started_at.clone());
    }
    emit_status(&app, "starting", "麦克风启动中，请稍候", None, None);

    let recorder = match recorder::ActiveShortRecorder::start().await {
        Ok(value) => {
            log::info!("Voice input recorder started");
            value
        }
        Err(error) => {
            log::error!("Voice input recorder failed to start: {error}");
            reset_to_idle();
            emit_status(&app, "failed", &error, None, None);
            return Err(error);
        }
    };
    let listening_message = listening_status_message(&hotkey_label, false);
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
        let updated_listening_message = listening_status_message(&hotkey_label, true);
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
    Ok(status())
}

pub async fn stop_dictation(app: AppHandle) -> Result<VoiceInputDictationResult, String> {
    log::info!("Voice input stop requested");
    let stop_started = Instant::now();
    let (recorder, enter_submit) = {
        let mut state = STATE
            .lock()
            .map_err(|e| format!("Failed to lock voice input state: {e}"))?;
        if state.phase != VoiceInputPhase::Listening {
            return Err("语音输入法当前没有在听写".to_string());
        }
        state.phase = VoiceInputPhase::Transcribing;
        state.enter_submit_available = false;
        (state.recorder.take(), state.enter_submit.take())
    };
    drop(enter_submit);
    let Some(recorder) = recorder else {
        reset_to_idle();
        return Err("语音输入法录音状态丢失".to_string());
    };

    emit_status(&app, "transcribing", "正在转写", None, None);
    let recorder_stop_started = Instant::now();
    let samples = match recorder.stop().await {
        Ok(value) => {
            log::info!(
                "Voice input recorder stopped: samples={} duration_ms={} recorder_stop_ms={}",
                value.len(),
                audio_duration_ms(value.len()),
                recorder_stop_started.elapsed().as_millis()
            );
            value
        }
        Err(error) => {
            log::error!(
                "Voice input recorder failed to stop after {} ms: {error}",
                recorder_stop_started.elapsed().as_millis()
            );
            reset_to_idle();
            emit_status(&app, "failed", &error, None, None);
            return Err(error);
        }
    };
    if samples.len() < MIN_AUDIO_SAMPLES {
        let error = format!(
            "语音太短（{}ms），请至少录制 {}ms",
            audio_duration_ms(samples.len()),
            audio_duration_ms(MIN_AUDIO_SAMPLES)
        );
        log::warn!(
            "Voice input rejected short audio: samples={} min_samples={}",
            samples.len(),
            MIN_AUDIO_SAMPLES
        );
        reset_to_idle();
        emit_status(&app, "failed", &error, None, None);
        return Err(error);
    }

    let result = process_samples(app.clone(), samples).await;
    reset_to_idle();
    match result {
        Ok(result) => {
            log::info!(
                "Voice input completed: inserted={} strategy={} raw_chars={} final_chars={} stop_total_ms={}",
                result.inserted,
                result.insertion_strategy,
                text::count_inserted_chars(&result.raw_text),
                text::count_inserted_chars(&result.text),
                stop_started.elapsed().as_millis()
            );
            Ok(result)
        }
        Err(error) => {
            log::error!("Voice input failed: {error}");
            emit_status(&app, "failed", &error, None, None);
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
        VoiceInputPhase::Starting => Ok(status()),
        _ => start_dictation(app).await,
    }
}

async fn process_samples(
    app: AppHandle,
    samples: Vec<f32>,
) -> Result<VoiceInputDictationResult, String> {
    let total_started = Instant::now();
    let settings = crate::db::get_settings()?;
    let temp_dir = crate::storage::get_temp_dir()?.join("voice-input");
    let id = Uuid::new_v4().to_string();
    let audio_path = temp_dir.join(format!("{id}.wav"));
    let normalized_path = temp_dir.join(format!("{id}.normalized.wav"));
    log::info!(
        "Voice input processing audio: samples={} duration_ms={} wav_path={}",
        samples.len(),
        audio_duration_ms(samples.len()),
        audio_path.display()
    );
    let write_started = Instant::now();
    if let Err(error) = recorder::write_wav(&audio_path, &samples) {
        log::error!(
            "Voice input WAV write failed: id={} elapsed_ms={} error={}",
            id,
            write_started.elapsed().as_millis(),
            error
        );
        return Err(error);
    }
    let write_wav_ms = write_started.elapsed().as_millis();
    log::info!(
        "Voice input WAV write completed: id={} elapsed_ms={} path={}",
        id,
        write_wav_ms,
        audio_path.display()
    );

    let asr_started = Instant::now();
    let raw_text = transcribe_short_audio(&app, &settings, &audio_path, &normalized_path).await?;
    let asr_ms = asr_started.elapsed().as_millis();
    log::info!(
        "Voice input ASR text received: id={} elapsed_ms={} {}",
        id,
        asr_ms,
        text::debug_text_summary("raw", &raw_text)
    );
    let mut polish_ms = 0;
    let polish_outcome = if settings.voice_input_refinement_mode == "ai_polish" {
        set_phase(VoiceInputPhase::Refining);
        emit_status(&app, "refining", "正在润色", None, None);
        log::info!("Voice input AI polish started");
        let api_key = crate::db::get_llm_api_key()?;
        let started = Instant::now();
        let outcome = voice_input_text_after_polish(
            &raw_text,
            crate::llm::polish_voice_input_text(&raw_text, &settings, &api_key).await,
        );
        polish_ms = started.elapsed().as_millis();
        if outcome.fallback {
            log::warn!(
                "Voice input AI polish failed after {} ms, using raw transcript: {}",
                polish_ms,
                outcome.error.as_deref().unwrap_or("unknown error")
            );
            emit_status(&app, "refining", "润色失败，已使用原始转写", None, None);
        } else {
            log::info!(
                "Voice input AI polish completed after {} ms: {} {}",
                polish_ms,
                text::debug_text_summary("raw", &raw_text),
                text::debug_text_summary("polished", &outcome.text)
            );
        }
        outcome
    } else {
        VoiceInputPolishOutcome {
            text: raw_text.clone(),
            fallback: false,
            error: None,
        }
    };
    let text = polish_outcome.text.clone();

    set_phase(VoiceInputPhase::Inserting);
    emit_status(&app, "inserting", "正在写入", None, None);
    log::info!(
        "Voice input insertion started: {}",
        text::debug_text_summary("final", &text)
    );
    let insertion_started = Instant::now();
    let insertion = insertion::insert_text(&text)?;
    let insertion_ms = insertion_started.elapsed().as_millis();
    log::info!(
        "Voice input insertion finished: inserted={} strategy={} clipboard_left_text={} elapsed_ms={} message={}",
        insertion.inserted,
        insertion.strategy,
        insertion.clipboard_left_text,
        insertion_ms,
        insertion.message
    );
    let stats_started = Instant::now();
    let stats = if insertion.inserted {
        let stats = crate::db::record_voice_input_success(&text)?;
        emit_status(
            &app,
            "inserted",
            &insertion.message,
            Some(text::count_inserted_chars(&text)),
            Some(insertion.strategy.clone()),
        );
        stats
    } else {
        emit_status(
            &app,
            "copied",
            &insertion.message,
            Some(text::count_inserted_chars(&text)),
            Some(insertion.strategy.clone()),
        );
        crate::db::get_voice_input_stats()?
    };
    let stats_ms = stats_started.elapsed().as_millis();

    let cleanup_started = Instant::now();
    let _ = std::fs::remove_file(&audio_path);
    let _ = std::fs::remove_file(&normalized_path);
    let cleanup_ms = cleanup_started.elapsed().as_millis();

    log::info!(
        "Voice input timing summary: id={} audio_duration_ms={} write_wav_ms={} asr_ms={} polish_ms={} insertion_ms={} stats_ms={} cleanup_ms={} total_ms={} polish_fallback={}",
        id,
        audio_duration_ms(samples.len()),
        write_wav_ms,
        asr_ms,
        polish_ms,
        insertion_ms,
        stats_ms,
        cleanup_ms,
        total_started.elapsed().as_millis(),
        polish_outcome.fallback
    );

    Ok(VoiceInputDictationResult {
        raw_text,
        text,
        inserted: insertion.inserted,
        insertion_strategy: insertion.strategy,
        message: insertion.message,
        polish_fallback: polish_outcome.fallback,
        polish_error: polish_outcome.error,
        stats,
    })
}

pub(crate) fn voice_input_text_after_polish(
    raw_text: &str,
    polish_result: Result<String, String>,
) -> VoiceInputPolishOutcome {
    match polish_result {
        Ok(text) => VoiceInputPolishOutcome {
            text,
            fallback: false,
            error: None,
        },
        Err(error) => VoiceInputPolishOutcome {
            text: raw_text.to_string(),
            fallback: true,
            error: Some(error),
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
    audio_path: &std::path::Path,
    normalized_path: &std::path::Path,
) -> Result<String, String> {
    let total_started = Instant::now();
    set_phase(VoiceInputPhase::PreparingModel);
    emit_status(
        app,
        "preparing_model",
        "正在准备 ASR 模型，首次使用可能需要下载",
        None,
        None,
    );
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
    set_phase(VoiceInputPhase::Transcribing);
    emit_status(app, "transcribing", "正在转写", None, None);
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
    let mut sidecar_future = Box::pin(crate::asr_worker::transcribe(app, request, settings));
    let response = tokio::select! {
        result = &mut sidecar_future => {
            match result {
                Ok(value) => {
                    log::info!(
                        "Voice input ASR sidecar completed: id={} elapsed_ms={}",
                        request_id,
                        sidecar_started.elapsed().as_millis()
                    );
                    value
                }
                Err(error) => {
                    log::error!(
                        "Voice input ASR sidecar failed: id={} elapsed_ms={} error={}",
                        request_id,
                        sidecar_started.elapsed().as_millis(),
                        error
                    );
                    return Err(error);
                }
            }
        }
        _ = tokio::time::sleep(Duration::from_secs(VOICE_INPUT_ASR_TIMEOUT_SECS)) => {
            log::error!(
                "Voice input ASR sidecar timed out: id={} elapsed_ms={}",
                request_id,
                sidecar_started.elapsed().as_millis()
            );
            return Err(format!(
                "语音输入 ASR 超时（{} 秒），请查看 npm run tauri dev 终端日志",
                VOICE_INPUT_ASR_TIMEOUT_SECS
            ));
        }
    };
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
                let refresh_result = match crate::db::get_settings() {
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
            HotkeyCommand::Triggered => {
                let app_for_task = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = toggle_dictation(app_for_task.clone()).await {
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
    if !enabled {
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
    let _ = send_hotkey_command(HotkeyCommand::Triggered);
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
        state.enter_submit.take()
    } else {
        None
    };
    drop(enter_submit);
}

fn set_phase(phase: VoiceInputPhase) {
    if let Ok(mut state) = STATE.lock() {
        state.phase = phase;
    }
}

fn emit_status(
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
            listening_status_message(&state.hotkey_label, state.enter_submit_available)
        }
        VoiceInputPhase::PreparingModel => "正在准备 ASR 模型".to_string(),
        VoiceInputPhase::Transcribing => "正在转写".to_string(),
        VoiceInputPhase::Refining => "正在润色".to_string(),
        VoiceInputPhase::Inserting => "正在写入".to_string(),
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

#[cfg(target_os = "macos")]
fn accessibility_trusted() -> bool {
    mod ffi {
        use std::ffi::c_void;

        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            pub fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        }
    }
    unsafe { ffi::AXIsProcessTrustedWithOptions(std::ptr::null()) }
}

#[cfg(not(target_os = "macos"))]
fn accessibility_trusted() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn request_accessibility_trust_prompt() -> bool {
    mod ffi {
        use std::ffi::c_void;

        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            pub static kAXTrustedCheckOptionPrompt: *const c_void;
            pub fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        }

        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            pub static kCFBooleanTrue: *const c_void;
            pub fn CFDictionaryCreate(
                allocator: *const c_void,
                keys: *const *const c_void,
                values: *const *const c_void,
                num_values: isize,
                key_callbacks: *const c_void,
                value_callbacks: *const c_void,
            ) -> *const c_void;
            pub fn CFRelease(cf: *const c_void);
        }
    }

    unsafe {
        let key = ffi::kAXTrustedCheckOptionPrompt;
        let value = ffi::kCFBooleanTrue;
        let options = ffi::CFDictionaryCreate(
            std::ptr::null(),
            &key,
            &value,
            1,
            std::ptr::null(),
            std::ptr::null(),
        );
        let trusted = ffi::AXIsProcessTrustedWithOptions(options);
        if !options.is_null() {
            ffi::CFRelease(options);
        }
        trusted
    }
}

#[cfg(not(target_os = "macos"))]
fn request_accessibility_trust_prompt() -> bool {
    false
}
