//! Path-based object storage: the few operations a new cloud backend has to
//! implement. [`super::repo::Repo`] builds the Ludonimbus layout (snapshots,
//! devices, game definitions) on top of any `Store`.
//!
//! Paths are `/`-separated and relative to the store's root folder, e.g.
//! `games/fe/snapshots/01J….zip`. They never start or end with `/`.

use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File or folder name (last path segment).
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Changes whenever the content changes (ETag, or mtime + size).
    pub tag: String,
}

pub trait Store: Send + Sync {
    /// Short machine name, e.g. `webdav`.
    fn kind(&self) -> &'static str;
    /// Human-readable location shown in the settings page.
    fn label(&self) -> String;
    /// `(used, total)` bytes when the backend reports them.
    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        Ok((None, None))
    }
    /// Make sure the root folder is reachable, creating it when missing.
    /// Connection problems must be reported as [`super::Unavailable`].
    fn check(&self) -> Result<()>;
    /// Direct children of `dir` (`""` = root). A missing folder is empty.
    fn list(&self, dir: &str) -> Result<Vec<Entry>>;
    /// Whole content of a small file; `None` when it does not exist.
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>>;
    /// Stream a file to `dest`. Errors when it does not exist.
    fn download(&self, path: &str, dest: &Path) -> Result<()>;
    /// Create or replace a file atomically (readers never see half a file),
    /// creating parent folders as needed.
    fn write(&self, path: &str, data: &[u8]) -> Result<()>;
    /// Like [`Store::write`], streaming from a local file.
    fn upload(&self, path: &str, src: &Path) -> Result<()>;
    /// Delete a file; deleting a missing file is not an error.
    fn remove(&self, path: &str) -> Result<()>;
    /// Identifies this store for the local metadata cache. `None` disables
    /// caching (cheap local folders).
    fn cache_scope(&self) -> Option<String> {
        None
    }
}

/// Join path segments with `/`, skipping empty ones.
pub fn join(parts: &[&str]) -> String {
    parts
        .iter()
        .flat_map(|p| p.split('/'))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Name of the temporary file used while uploading `path`.
pub fn temp_name(path: &str) -> String {
    format!("{path}.lntmp")
}
