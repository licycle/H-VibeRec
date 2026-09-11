//! Recording reservations are taken synchronously at shortcut entry. Waiting
//! deliveries never prevent another recording; an input dispatch already in
//! flight is atomic and must finish before a new target can be captured.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

#[derive(Default)]
pub(crate) struct FocusGate {
    state: Mutex<FocusState>,
    changed: Notify,
}

#[derive(Default)]
struct FocusState {
    recordings: usize,
    writing: bool,
    released_at: Option<Instant>,
}

pub(crate) struct RecordingLease(Arc<FocusGate>);
pub(crate) struct WriteLease(Arc<FocusGate>);

impl FocusGate {
    pub fn record(self: &Arc<Self>) -> Result<RecordingLease, String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if state.writing {
            return Err("正在完成一次位置恢复，请稍后再按录音快捷键".into());
        }
        state.recordings += 1;
        Ok(RecordingLease(self.clone()))
    }

    pub async fn write(self: &Arc<Self>, automatic: bool) -> WriteLease {
        loop {
            let changed = self.changed.notified();
            {
                let mut state = self.state.lock().unwrap();
                let quiet = !automatic
                    || state
                        .released_at
                        .map(|at| at.elapsed() >= Duration::from_millis(600))
                        .unwrap_or(true);
                if state.recordings == 0 && !state.writing && quiet {
                    state.writing = true;
                    return WriteLease(self.clone());
                }
            }
            tokio::select! {
                _ = changed => {},
                _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            }
        }
    }
}

impl Drop for RecordingLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.recordings -= 1;
            state.released_at = Some(Instant::now());
        }
        self.0.changed.notify_waiters();
    }
}
impl Drop for WriteLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.writing = false;
        }
        self.0.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn queued_delivery_yields_to_repeated_recordings_and_resumes_on_release() {
        let gate = Arc::new(FocusGate::default());
        let a = gate.record().unwrap();
        let mut delivery = Box::pin(gate.write(false));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut delivery)
                .await
                .is_err()
        );
        let b = gate.record().unwrap();
        drop(a);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut delivery)
                .await
                .is_err()
        );
        drop(b);
        let write = tokio::time::timeout(Duration::from_secs(1), delivery)
            .await
            .unwrap();
        assert!(gate.record().is_err());
        drop(write);
        assert!(gate.record().is_ok());
    }
}
