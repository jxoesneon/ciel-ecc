use anyhow::{Context, Result};
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static CURRENT_DIR_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub(crate) struct CurrentDirGuard {
        _lock: MutexGuard<'static, ()>,
        original_dir: PathBuf,
    }

    impl CurrentDirGuard {
        pub(crate) fn enter(target_dir: &Path) -> Result<Self> {
            let lock = CURRENT_DIR_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .expect("current-dir test lock poisoned");
            let original_dir =
                std::env::current_dir().context("Failed to capture current test directory")?;
            std::env::set_current_dir(target_dir).with_context(|| {
                format!("Failed to enter test directory {}", target_dir.display())
            })?;

            Ok(Self {
                _lock: lock,
                original_dir,
            })
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original_dir);
        }
    }
