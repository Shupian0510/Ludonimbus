//! Small files that were read once are not read again.
//!
//! [`Cached`] wraps any [`Store`]. A folder listing gives every file's
//! change tag. A file read while its tag is known is kept on this device,
//! and answered from there for as long as the listings keep showing that
//! tag. So the code above never has to think about it: list a folder, read
//! what it needs, and only what is new or changed costs a request.
//!
//! - A file whose folder was not listed by this process is read from the
//!   cloud: without a tag there is nothing to compare.
//! - Writing or removing a file through the wrapper forgets it.
//! - Stores without a [`Store::cache_scope`] (a local folder) are passed
//!   through.
//!
//! The files on disk are the ones 0.6 keeps (`cloud-cache/<store>/<path>`,
//! see [`super::repo::Repo`]): a device upgraded from 0.6 already has the
//! snapshots it has seen, and the migration doesn't fetch them again.

use super::store::{join, Entry, Store};
use crate::{paths, util};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Cached<S: Store> {
    inner: S,
    /// `None`: nothing is kept.
    dir: Option<PathBuf>,
    /// Path to the tag its folder's last listing showed.
    tags: Mutex<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize)]
struct Kept {
    tag: String,
    body: String,
}

fn hex(s: &str, len: usize) -> String {
    blake3::hash(s.as_bytes()).to_hex()[..len].to_string()
}

impl<S: Store> Cached<S> {
    /// Keep what is read in this device's data folder, apart for each store.
    pub fn new(inner: S) -> Self {
        let dir = inner.cache_scope().map(|scope| paths::data_dir().join("cloud-cache").join(hex(&scope, 16)));
        Cached { inner, dir, tags: Mutex::default() }
    }

    /// Keep what is read in `dir`, whatever the store says about caching.
    pub fn at(inner: S, dir: impl Into<PathBuf>) -> Self {
        Cached { inner, dir: Some(dir.into()), tags: Mutex::default() }
    }

    pub fn inner(&self) -> &S {
        &self.inner
    }

    fn file(&self, path: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{}.json", hex(path, 32))))
    }

    fn forget(&self, path: &str) {
        self.tags.lock().unwrap().remove(path);
        if let Some(f) = self.file(path) {
            let _ = std::fs::remove_file(f);
        }
    }
}

impl<S: Store> Store for Cached<S> {
    fn kind(&self) -> &'static str {
        self.inner.kind()
    }

    fn label(&self) -> String {
        self.inner.label()
    }

    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        self.inner.quota()
    }

    fn check(&self) -> Result<()> {
        self.inner.check()
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        let entries = self.inner.list(dir)?;
        if self.dir.is_some() {
            let mut tags = self.tags.lock().unwrap();
            // What the folder no longer has, or has without a tag, is not
            // known any more.
            tags.retain(|path, _| path.rsplit_once('/').map_or("", |(parent, _)| parent) != dir);
            for e in entries.iter().filter(|e| !e.is_dir && !e.tag.is_empty()) {
                tags.insert(join(&[dir, &e.name]), e.tag.clone());
            }
        }
        Ok(entries)
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let tag = self.tags.lock().unwrap().get(path).cloned();
        let (Some(tag), Some(file)) = (tag, self.file(path)) else {
            return self.inner.read(path);
        };
        if let Some(kept) = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Kept>(&b).ok()) {
            if kept.tag == tag {
                return Ok(Some(kept.body.into_bytes()));
            }
        }
        let body = self.inner.read(path)?;
        if let Some(text) = body.as_deref().and_then(|b| std::str::from_utf8(b).ok()) {
            if let Ok(bytes) = serde_json::to_vec(&Kept { tag, body: text.to_string() }) {
                let _ = util::atomic_write(&file, &bytes);
            }
        }
        Ok(body)
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.inner.download(path, dest)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.forget(path);
        self.inner.write(path, data)
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.forget(path);
        self.inner.upload(path, src)
    }

    fn remove(&self, path: &str) -> Result<()> {
        self.forget(path);
        self.inner.remove(path)
    }

    fn cache_scope(&self) -> Option<String> {
        self.inner.cache_scope()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::memory::Memory;

    fn reads(store: &Cached<Memory>) -> u32 {
        store.inner().take().read
    }

    #[test]
    fn a_file_is_read_once_per_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Cached::at(Memory::new(), tmp.path());
        store.inner().put("games/a/saves/main/01.json", b"one");
        store.inner().put("games/a/saves/main/02.json", b"two");

        // Not listed yet: there is no tag to go by.
        assert_eq!(store.read("games/a/saves/main/01.json").unwrap().unwrap(), b"one");
        assert_eq!(store.read("games/a/saves/main/01.json").unwrap().unwrap(), b"one");
        assert_eq!(reads(&store), 2);

        store.list("games/a/saves/main").unwrap();
        assert_eq!(store.read("games/a/saves/main/01.json").unwrap().unwrap(), b"one");
        assert_eq!(store.read("games/a/saves/main/01.json").unwrap().unwrap(), b"one");
        assert_eq!(store.read("games/a/saves/main/02.json").unwrap().unwrap(), b"two");
        assert_eq!(reads(&store), 2);

        // Another process, later: the listing is all it takes.
        let again = Cached::at(Memory::new(), tmp.path());
        again.inner().put("games/a/saves/main/01.json", b"one");
        // (a new `Memory` hands out the same first tag for the same file)
        again.list("games/a/saves/main").unwrap();
        assert_eq!(again.read("games/a/saves/main/01.json").unwrap().unwrap(), b"one");
        assert_eq!(reads(&again), 0);
    }

    #[test]
    fn a_changed_file_is_read_again() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Cached::at(Memory::new(), tmp.path());
        store.inner().put("library/g.json", b"rev 1");
        store.list("library").unwrap();
        assert_eq!(store.read("library/g.json").unwrap().unwrap(), b"rev 1");

        // Another device rewrites it. Until the folder is listed again this
        // device goes by the listing it has.
        store.inner().put("library/g.json", b"rev 2");
        assert_eq!(store.read("library/g.json").unwrap().unwrap(), b"rev 1");
        store.list("library").unwrap();
        assert_eq!(store.read("library/g.json").unwrap().unwrap(), b"rev 2");
        assert_eq!(store.read("library/g.json").unwrap().unwrap(), b"rev 2");
        assert_eq!(reads(&store), 2);
    }

    #[test]
    fn what_this_device_writes_or_removes_is_forgotten() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Cached::at(Memory::new(), tmp.path());
        store.inner().put("d/x.json", b"old");
        store.inner().put("d/y.json", b"y");
        store.list("d").unwrap();
        store.read("d/x.json").unwrap();
        store.read("d/y.json").unwrap();
        reads(&store);

        store.write("d/x.json", b"new").unwrap();
        assert_eq!(store.read("d/x.json").unwrap().unwrap(), b"new");
        store.remove("d/y.json").unwrap();
        assert_eq!(store.read("d/y.json").unwrap(), None);
        assert_eq!(reads(&store), 2);

        // A file that left the folder is no longer answered from here, also
        // when another device removed it.
        store.inner().put("d/z.json", b"z");
        store.list("d").unwrap();
        store.read("d/z.json").unwrap();
        store.inner().remove("d/z.json").unwrap();
        store.list("d").unwrap();
        assert_eq!(store.read("d/z.json").unwrap(), None);
    }

    #[test]
    fn a_store_that_asks_for_no_cache_is_passed_through() {
        // `Memory` has no cache scope, like a local folder.
        let store = Cached::new(Memory::new());
        store.inner().put("d/x.json", b"x");
        store.list("d").unwrap();
        store.read("d/x.json").unwrap();
        store.read("d/x.json").unwrap();
        assert_eq!(reads(&store), 2);
    }

    #[test]
    fn folders_are_told_apart() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Cached::at(Memory::new(), tmp.path());
        store.inner().put("a/x.json", b"in a");
        store.inner().put("a/b/x.json", b"in a/b");
        store.list("a").unwrap();
        store.list("a/b").unwrap();
        store.read("a/x.json").unwrap();
        store.read("a/b/x.json").unwrap();
        reads(&store);
        // Listing one folder again says nothing about the other.
        store.list("a/b").unwrap();
        assert_eq!(store.read("a/x.json").unwrap().unwrap(), b"in a");
        assert_eq!(store.read("a/b/x.json").unwrap().unwrap(), b"in a/b");
        assert_eq!(reads(&store), 0);
    }
}
