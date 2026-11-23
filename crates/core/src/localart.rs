//! Artwork chosen in Ludonimbus (`%APPDATA%\Ludonimbus\art\<game>\<kind>.<ext>`).
//!
//! Games that are not in Steam show it in the library; "Add to Steam" copies it
//! into Steam's grid folder.

use crate::{paths, steam};
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

const EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

pub fn dir(key: &str) -> PathBuf {
    paths::data_dir().join("art").join(key)
}

pub fn file(key: &str, kind: &str) -> Option<PathBuf> {
    let d = dir(key);
    EXTS.iter().map(|e| d.join(format!("{kind}.{e}"))).find(|p| p.is_file())
}

/// `(kind, file)` for every kind that has an image.
pub fn all(key: &str) -> Vec<(String, PathBuf)> {
    steam::ART_KINDS
        .iter()
        .filter_map(|k| file(key, k).map(|f| (k.to_string(), f)))
        .collect()
}

/// Store an image as the game's artwork of one kind (replacing the old one).
pub fn set(key: &str, kind: &str, src: &Path) -> Result<PathBuf> {
    if !steam::ART_KINDS.contains(&kind) {
        return Err(anyhow!(crate::tr!("未知的图片类型 {kind}", "Unknown image type {kind}")));
    }
    let d = dir(key);
    std::fs::create_dir_all(&d)?;
    clear(key, kind);
    let dest = d.join(format!("{kind}.{}", steam::image_ext(src)));
    std::fs::copy(src, &dest).with_context(|| crate::tr!("无法保存图片 {}", "Couldn't save the image {}", dest.display()))?;
    Ok(dest)
}

pub fn clear(key: &str, kind: &str) {
    let d = dir(key);
    for e in EXTS {
        let _ = std::fs::remove_file(d.join(format!("{kind}.{e}")));
    }
}

pub fn remove_all(key: &str) {
    let _ = std::fs::remove_dir_all(dir(key));
}
