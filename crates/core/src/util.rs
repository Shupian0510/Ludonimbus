use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Write a file atomically: write to a temp sibling then rename over the target.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("无法创建目录 {}", parent.display()))?;
    }
    let tmp = path.with_extension(format!(
        "{}.lntmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, bytes).with_context(|| format!("无法写入 {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("无法替换 {}", path.display()))?;
    Ok(())
}

/// Case-insensitive path equality (Windows semantics), ignoring slash style
/// and surrounding quotes.
pub fn same_path(a: &str, b: &str) -> bool {
    norm_path(a) == norm_path(b)
}

pub fn norm_path(p: &str) -> String {
    p.trim()
        .trim_matches('"')
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Remove one level of surrounding double quotes.
pub fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.trim_matches('"'))
}

/// File name of a path, lowercased.
pub fn file_name_lower(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Split a Windows-style command line into arguments (handles double quotes).
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'"') => {
                chars.next();
                cur.push('"');
                has_token = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

/// Pick the first existing file among candidates.
pub fn first_existing(cands: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    cands.into_iter().find(|p| p.is_file())
}

/// Find a file in `dir` whose name matches `name` case-insensitively.
pub fn find_file_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let want = name.to_lowercase();
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let p = e.path();
        if p.is_file() && e.file_name().to_string_lossy().to_lowercase() == want {
            Some(p)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_args() {
        assert_eq!(split_args(r#"--a "b c" d"#), vec!["--a", "b c", "d"]);
        assert_eq!(split_args(""), Vec::<String>::new());
        assert_eq!(split_args(r#""""#), vec![""]);
    }

    #[test]
    fn compares_paths() {
        assert!(same_path(r#""C:\Games\x.exe""#, "c:/games/X.EXE"));
        assert!(!same_path(r"C:\a", r"C:\b"));
    }
}
