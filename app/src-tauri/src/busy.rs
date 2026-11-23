//! Games this app is syncing right now, so the UI, the local API and the
//! Steam plugin never start two syncs of the same game at once.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

fn set() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Marks a game as syncing until dropped.
pub struct Guard(String);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut s) = set().lock() {
            s.remove(&self.0);
        }
    }
}

/// `None` when the game is already being synced.
pub fn begin(key: &str) -> Option<Guard> {
    let mut s = set().lock().ok()?;
    s.insert(key.to_string()).then(|| Guard(key.to_string()))
}

pub fn is_busy(key: &str) -> bool {
    set().lock().map(|s| s.contains(key)).unwrap_or(false)
}
