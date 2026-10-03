//! One Git operation at a time per repository in this process. Repositories are
//! read and changed in place, and runs sharing a named volume can reach one
//! repository at once.
use crate::runtime::fs::FileIdentity;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;
use wasmtime::{Result, bail};

#[derive(Default)]
struct Slot {
    held: Mutex<bool>,
    released: Condvar,
}

static SLOTS: Mutex<Option<HashMap<FileIdentity, Weak<Slot>>>> = Mutex::new(None);

/// Holds a repository, identified by its directory, until dropped.
pub(super) struct RepositoryLock {
    slot: Arc<Slot>,
}

impl RepositoryLock {
    /// Waits for the repository to be free, giving up once `cancelled` is set.
    pub(super) fn acquire(repository: FileIdentity, cancelled: &AtomicBool) -> Result<Self> {
        let slot = {
            let mut slots = SLOTS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let slots = slots.get_or_insert_with(HashMap::new);
            slots.retain(|_, slot| slot.strong_count() > 0);
            if let Some(slot) = slots.get(&repository).and_then(Weak::upgrade) {
                slot
            } else {
                let slot = Arc::new(Slot::default());
                slots.insert(repository, Arc::downgrade(&slot));
                slot
            }
        };
        let mut held = slot
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *held {
            if cancelled.load(Ordering::Relaxed) {
                bail!("git: operation cancelled");
            }
            held = slot
                .released
                .wait_timeout(held, Duration::from_millis(10))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        *held = true;
        drop(held);
        Ok(Self { slot })
    }
}

impl Drop for RepositoryLock {
    fn drop(&mut self) {
        *self
            .slot
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
        self.slot.released.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_one_repository_and_gives_up_when_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let root =
            cap_std::fs::Dir::open_ambient_dir(dir.path(), cap_std::ambient_authority()).unwrap();
        let identity = FileIdentity::of(&root.dir_metadata().unwrap()).unwrap();
        let first = RepositoryLock::acquire(identity, &AtomicBool::new(false)).unwrap();
        let error = RepositoryLock::acquire(identity, &AtomicBool::new(true))
            .err()
            .unwrap();
        assert!(error.to_string().contains("cancelled"));
        let waiter = std::thread::spawn(move || {
            RepositoryLock::acquire(identity, &AtomicBool::new(false)).map(drop)
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(!waiter.is_finished());
        drop(first);
        waiter.join().unwrap().unwrap();
    }
}
