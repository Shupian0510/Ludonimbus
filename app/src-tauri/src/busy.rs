//! Games this app is syncing right now, so the UI, the local API and the
//! Steam plugin never start two syncs of the same game at once.

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// The set stays whole when a thread panicked while holding it (every change
/// is one insert or remove), so a panic never leaves a game marked busy.
fn set() -> MutexGuard<'static, HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap_or_else(|e| e.into_inner())
}

/// Marks a game as syncing until dropped.
pub struct Guard(String);

impl Drop for Guard {
    fn drop(&mut self) {
        set().remove(&self.0);
    }
}

/// `None` when the game is already being synced.
pub fn begin(key: &str) -> Option<Guard> {
    set().insert(key.to_string()).then(|| Guard(key.to_string()))
}

pub fn is_busy(key: &str) -> bool {
    set().contains(key)
}
