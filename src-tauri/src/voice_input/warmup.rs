use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::commands::local::dictation_model_paths_if_ready_for_queue;
use crate::types::{AppSettings, VoiceInputWarmupStatusEvent};

const VOICE_INPUT_STARTUP_WARMUP_DELAY_SECS: u64 = 3;
const VOICE_INPUT_WARMUP_AUDIO_SECONDS: f32 = 0.5;
const VOICE_INPUT_WARMUP_AUDIO_HZ: f32 = 440.0;

pub(super) fn schedule_startup_dictation_warmup(app: AppHandle) {
    schedule_dictation_warmup(app, "startup", VOICE_INPUT_STARTUP_WARMUP_DELAY_SECS);
}

pub(crate) fn build_dictation_warmup_request(
    id: &str,
    model_path: &str,
    use_gpu: bool,
    punc_model_path: &str,
    audio_path: &str,
) -> Value {
    json!({
        "id": id,
        "type": "warmup",
        "payload": {
            "audio_path": audio_path,
            "model_path": model_path,
            "use_gpu": use_gpu,
            "punc_model_path": punc_model_path,
            "profile": "dictation",
            "audio_already_normalized": true,
            "reuse_model": true
        }
    })
}

pub(crate) fn should_startup_dictation_warmup(
    settings: &AppSettings,
    meeting_recording_active: bool,
) -> bool {
    settings.voice_input_enabled && !meeting_recording_active
}

pub(crate) fn should_schedule_dictation_warmup_after_settings_change(
    previous: &AppSettings,
    saved: &AppSettings,
) -> bool {
    saved.voice_input_enabled
        && (!previous.voice_input_enabled
            || previous.asr_model_repo != saved.asr_model_repo
            || previous.asr_model_source != saved.asr_model_source
            || previous.asr_model_path != saved.asr_model_path
            || previous.use_gpu != saved.use_gpu)
}

pub(crate) fn build_warmup_status_event(
    phase: &str,
    message: &str,
    reason: &str,
    elapsed_ms: Option<i64>,
    sidecar_infer_ms: Option<i64>,
) -> VoiceInputWarmupStatusEvent {
    VoiceInputWarmupStatusEvent {
        phase: phase.to_string(),
        message: message.to_string(),
        reason: reason.to_string(),
        elapsed_ms,
        sidecar_infer_ms,
    }
}

pub(crate) fn schedule_dictation_warmup(app: AppHandle, reason: &'static str, delay_secs: u64) {
    log::info!(
        "Voice input ASR warmup scheduled: reason={} delay_secs={}",
        reason,
        delay_secs
    );
    let builder = thread::Builder::new().name(format!("voice-input-warmup-{reason}"));
    if let Err(error) = builder.spawn(move || {
        if delay_secs > 0 {
            log::info!(
                "Voice input ASR warmup waiting before trigger: reason={} delay_secs={}",
                reason,
                delay_secs
            );
            thread::sleep(Duration::from_secs(delay_secs));
        }
        log::info!("Voice input ASR warmup trigger fired: reason={reason}");
        tauri::async_runtime::block_on(async move {
            if let Err(error) = warmup_dictation_worker(app, reason).await {
                log::warn!(
                    "Voice input ASR warmup skipped or failed: reason={} error={}",
                    reason,
                    error
                );
            }
        });
    }) {
        log::warn!("Voice input ASR warmup could not be scheduled: reason={reason} error={error}");
    }
}

async fn warmup_dictation_worker(app: AppHandle, reason: &'static str) -> Result<(), String> {
    let settings = crate::db::get_settings()?;
    let meeting_recording_active = crate::recording::is_recording();
    if !should_startup_dictation_warmup(&settings, meeting_recording_active) {
        log::info!(
            "Voice input ASR warmup skipped: reason={} enabled={} meeting_recording_active={}",
            reason,
            settings.voice_input_enabled,
            meeting_recording_active
        );
        return Ok(());
    }
    if !super::is_idle_phase() {
        log::info!("Voice input ASR warmup skipped: reason={reason} voice input is not idle");
        return Ok(());
    }

    let Some((model_path, punc_model_path)) = dictation_model_paths_if_ready_for_queue(&settings)?
    else {
        log::info!(
            "Voice input ASR warmup skipped: reason={} dictation models are not ready",
            reason
        );
        emit_warmup_status(
            &app,
            "skipped",
            "本地转写模型尚未准备完成",
            reason,
            None,
            None,
        );
        return Ok(());
    };

    let temp_dir = crate::storage::get_temp_dir()?.join("voice-input");
    let warmup_audio_path = temp_dir.join("dictation-warmup-16k.wav");
    let warmup_samples = dictation_warmup_samples();
    super::recorder::write_wav(&warmup_audio_path, &warmup_samples)?;
    emit_warmup_status(
        &app,
        "warming",
        "正在预热本地转写模型，首次语音输入可能需要几秒",
        reason,
        None,
        None,
    );

    let request_id = format!("voice-input-warmup-{}", Uuid::new_v4());
    let request = build_dictation_warmup_request(
        &request_id,
        &model_path,
        settings.use_gpu,
        &punc_model_path.to_string_lossy(),
        &warmup_audio_path.to_string_lossy(),
    );
    log::info!(
        "Voice input ASR warmup started: reason={} id={} audio={} model={} punc={} use_gpu={}",
        reason,
        request_id,
        warmup_audio_path.display(),
        model_path,
        punc_model_path.display(),
        settings.use_gpu
    );
    let started = Instant::now();
    let response = crate::asr_worker::transcribe(&app, request, &settings).await;
    let _ = std::fs::remove_file(&warmup_audio_path);
    let response = match response {
        Ok(value) => value,
        Err(error) => {
            emit_warmup_status(
                &app,
                "skipped",
                "本地转写模型预热未完成，首次语音输入可能仍需等待",
                reason,
                Some(started.elapsed().as_millis() as i64),
                None,
            );
            return Err(error);
        }
    };
    let total_warmup_ms = response
        .pointer("/result/timing/total_warmup_ms")
        .and_then(|value| value.as_i64());
    let warmup_infer_ms = response
        .pointer("/result/timing/warmup_infer_ms")
        .and_then(|value| value.as_i64());
    log::info!(
        "Voice input ASR warmup completed: reason={} id={} elapsed_ms={} sidecar_total_warmup_ms={:?} sidecar_warmup_infer_ms={:?}",
        reason,
        request_id,
        started.elapsed().as_millis(),
        total_warmup_ms,
        warmup_infer_ms
    );
    emit_warmup_status(
        &app,
        "ready",
        "本地转写模型已就绪",
        reason,
        total_warmup_ms.or(Some(started.elapsed().as_millis() as i64)),
        warmup_infer_ms,
    );
    Ok(())
}

fn emit_warmup_status(
    app: &AppHandle,
    phase: &str,
    message: &str,
    reason: &str,
    elapsed_ms: Option<i64>,
    sidecar_infer_ms: Option<i64>,
) {
    log::info!(
        "Voice input warmup status event: phase={} reason={} message={} elapsed_ms={:?} sidecar_infer_ms={:?}",
        phase,
        reason,
        message,
        elapsed_ms,
        sidecar_infer_ms
    );
    let _ = app.emit(
        "voice-input-warmup-status",
        build_warmup_status_event(phase, message, reason, elapsed_ms, sidecar_infer_ms),
    );
}

fn dictation_warmup_samples() -> Vec<f32> {
    let sample_count =
        (crate::audio::TARGET_SAMPLE_RATE as f32 * VOICE_INPUT_WARMUP_AUDIO_SECONDS) as usize;
    (0..sample_count)
        .map(|index| {
            let seconds = index as f32 / crate::audio::TARGET_SAMPLE_RATE as f32;
            (seconds * VOICE_INPUT_WARMUP_AUDIO_HZ * std::f32::consts::TAU).sin() * 0.05
        })
        .collect()
}
