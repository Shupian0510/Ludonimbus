//! A store kept in memory that counts its requests. For tests: how many
//! requests an operation costs is part of the design (docs/plan-v2.md,
//! section 5.6), and a cloud that answers in seconds makes every extra one
//! felt.

use super::store::{Entry, Store};
use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

/// Requests made since the store was created or [`Memory::take`] was called.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub list: u32,
    pub read: u32,
    pub download: u32,
    pub write: u32,
    pub upload: u32,
    pub remove: u32,
}

impl Counts {
    pub fn total(&self) -> u32 {
        self.list + self.read + self.download + self.write + self.upload + self.remove
    }
}

#[derive(Default)]
struct Inner {
    /// path -> (content, tag)
    files: BTreeMap<String, (Vec<u8>, u64)>,
    clock: u64,
    counts: Counts,
}

#[derive(Default)]
pub struct Memory {
    inner: Mutex<Inner>,
}

impl Memory {
    pub fn new() -> Self {
        Memory::default()
    }

    pub fn counts(&self) -> Counts {
        self.inner.lock().unwrap().counts
    }

    /// The counts so far, starting again from zero.
    pub fn take(&self) -> Counts {
        std::mem::take(&mut self.inner.lock().unwrap().counts)
    }

    /// Every file's path, sorted.
    pub fn paths(&self) -> Vec<String> {
        self.inner.lock().unwrap().files.keys().cloned().collect()
    }

    /// Look at a file without counting a request.
    pub fn peek(&self, path: &str) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().files.get(path).map(|(data, _)| data.clone())
    }

    /// Put a file in place without counting a request.
    pub fn put(&self, path: &str, data: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        inner.clock += 1;
        let tag = inner.clock;
        inner.files.insert(path.to_string(), (data.to_vec(), tag));
    }
}

impl Store for Memory {
    fn kind(&self) -> &'static str {
        "memory"
    }

    fn label(&self) -> String {
        "memory".to_string()
    }

    fn check(&self) -> Result<()> {
        Ok(())
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        let mut inner = self.inner.lock().unwrap();
        inner.counts.list += 1;
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let mut out = Vec::new();
        let mut dirs = BTreeSet::new();
        for (path, (data, tag)) in &inner.files {
            let Some(rest) = path.strip_prefix(&prefix) else { continue };
            match rest.split_once('/') {
                Some((folder, _)) => {
                    if dirs.insert(folder.to_string()) {
                        out.push(Entry { name: folder.to_string(), is_dir: true, size: 0, tag: String::new() });
                    }
                }
                None => out.push(Entry {
                    name: rest.to_string(),
                    is_dir: false,
                    size: data.len() as u64,
                    tag: tag.to_string(),
                }),
            }
        }
        Ok(out)
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let mut inner = self.inner.lock().unwrap();
        inner.counts.read += 1;
        Ok(inner.files.get(path).map(|(data, _)| data.clone()))
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        let data = {
            let mut inner = self.inner.lock().unwrap();
            inner.counts.download += 1;
            inner.files.get(path).map(|(data, _)| data.clone())
        };
        let data = data.ok_or_else(|| anyhow!("{path} does not exist"))?;
        std::fs::write(dest, data)?;
        Ok(())
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.counts.write += 1;
        inner.clock += 1;
        let tag = inner.clock;
        inner.files.insert(path.to_string(), (data.to_vec(), tag));
        Ok(())
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        let data = std::fs::read(src)?;
        let mut inner = self.inner.lock().unwrap();
        inner.counts.upload += 1;
        inner.clock += 1;
        let tag = inner.clock;
        inner.files.insert(path.to_string(), (data, tag));
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.counts.remove += 1;
        inner.files.remove(path);
        Ok(())
    }
}
