#[cfg(test)]
pub(crate) mod support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static TEST_APP_DATA_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub(crate) fn lock_app_data() -> MutexGuard<'static, ()> {
        TEST_APP_DATA_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

mod db_queue;
mod voice_input;
