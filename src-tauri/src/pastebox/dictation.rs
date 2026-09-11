use std::{future::Future, time::Duration};

use tokio::sync::watch;

use crate::types::{PasteMode, PasteTarget};

/// One recording owns one capture result. Dropping a recording does not abort an
/// in-flight AX request (which would destroy the helper and its other targets).
#[derive(Clone)]
pub struct DictationContext {
    id: String,
    mode: PasteMode,
    capture: watch::Receiver<Option<Result<PasteTarget, String>>>,
    recording: std::sync::Arc<std::sync::Mutex<Option<super::focus::RecordingLease>>>,
}

impl std::fmt::Debug for DictationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DictationContext")
            .field("id", &self.id)
            .field("mode", &self.mode)
            .finish()
    }
}

impl DictationContext {
    pub(super) fn start(
        id: String,
        mode: PasteMode,
        capture: impl Future<Output = Result<PasteTarget, String>> + Send + 'static,
    ) -> Self {
        let (sender, receiver) = watch::channel(None);
        let job_id = id.clone();
        tauri::async_runtime::spawn(async move {
            let started = std::time::Instant::now();
            let result = tokio::time::timeout(Duration::from_secs(12), capture)
                .await
                .unwrap_or_else(|_| Err("记录原位置超时，内容将保留到粘贴箱".into()));
            match &result {
                Ok(target) => log::info!(
                    "Dictation capture ready: job={} target={} pid={:?} window_id={:?} identity={:?} role={:?} capability={:?} selection={}+{} elapsed_ms={}",
                    job_id, target.id, target.process_id, target.window_id, target.identity_method, target.control_role,
                    target.capability,
                    target.selection_location, target.selection_length, started.elapsed().as_millis()
                ),
                Err(error) => log::warn!(
                    "Dictation capture failed: job={} elapsed_ms={} error={error}",
                    job_id, started.elapsed().as_millis()
                ),
            }
            let _ = sender.send(Some(result));
        });
        Self {
            id,
            mode,
            capture: receiver,
            recording: Default::default(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub(super) fn with_recording_lease(self, lease: super::focus::RecordingLease) -> Self {
        *self.recording.lock().unwrap() = Some(lease);
        self
    }

    pub fn release_recording(&self) {
        if let Ok(mut lease) = self.recording.lock() {
            lease.take();
        }
    }

    pub fn mode(&self) -> PasteMode {
        self.mode
    }

    pub fn capture_error(&self) -> Option<String> {
        match self.capture.borrow().as_ref() {
            Some(Err(error)) => Some(error.clone()),
            None if self.capture.has_changed().is_err() => Some("位置采集任务已中断".into()),
            _ => None,
        }
    }

    pub async fn resolve(mut self) -> (PasteMode, Option<PasteTarget>) {
        loop {
            if let Some(result) = self.capture.borrow().clone() {
                return (self.mode, result.ok());
            }
            if self.capture.changed().await.is_err() {
                return (self.mode, None);
            }
        }
    }

    pub fn target_hint(&self) -> String {
        if self.mode == PasteMode::Manual {
            return "结果将保留到粘贴箱".into();
        }
        match self.capture.borrow().as_ref() {
            Some(Ok(target)) => format!("已记录 {} 的输入位置", target.app_name),
            Some(Err(_)) => "未记录到输入位置，结果将保留到粘贴箱".into(),
            None if self.capture.has_changed().is_err() => {
                "位置采集已中断，结果将保留到粘贴箱".into()
            }
            None => "正在记录原输入位置".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str) -> PasteTarget {
        serde_json::from_value(serde_json::json!({
            "id": id, "app_name": "fixture", "bundle_id": "fixture", "window_title": "original",
            "captured_at": "now", "available": true, "selection_location": 3, "selection_length": 0
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn slow_capture_does_not_block_transcription_but_delivery_waits() {
        let (release, waiting) = tokio::sync::oneshot::channel();
        let context = DictationContext::start("recording-a".into(), PasteMode::Automatic, async {
            waiting.await.unwrap();
            Ok(target("original"))
        });
        assert!(context.target_hint().contains("正在记录"));
        let mut delivery = Box::pin(context.resolve());
        let transcript = tokio::spawn(async { "transcript" }).await.unwrap();
        assert_eq!(transcript, "transcript");
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut delivery)
                .await
                .is_err()
        );
        release.send(()).unwrap();
        let (mode, target) = delivery.await;
        assert_eq!(mode, PasteMode::Automatic);
        assert_eq!(target.unwrap().id, "original");
    }

    #[tokio::test]
    async fn late_cancelled_capture_cannot_supply_the_next_recording() {
        let (release, waiting) = tokio::sync::oneshot::channel();
        let (finished, completion) = tokio::sync::oneshot::channel();
        let cancelled = DictationContext::start("cancelled".into(), PasteMode::Automatic, async {
            waiting.await.unwrap();
            finished.send(()).unwrap();
            Ok(target("old"))
        });
        drop(cancelled);
        let next = DictationContext::start("next".into(), PasteMode::Notify, async {
            Ok(target("new"))
        });
        release.send(()).unwrap();
        completion.await.unwrap();
        let (mode, target) = next.resolve().await;
        assert_eq!(mode, PasteMode::Notify);
        assert_eq!(target.unwrap().id, "new");
    }

    #[tokio::test]
    async fn capture_failure_preserves_the_recordings_mode_and_has_no_fallback_target() {
        let context = DictationContext::start("failed".into(), PasteMode::Notify, async {
            Err("capture unavailable".into())
        });
        let observer = context.clone();
        let (mode, target) = context.resolve().await;
        assert_eq!(mode, PasteMode::Notify);
        assert!(target.is_none());
        assert!(observer.target_hint().contains("未记录"));
    }
}
