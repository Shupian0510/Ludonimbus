//! Effective save specification of a game and per-device root resolution.
//!
//! Save locations are written as `{root}/relative/glob`. Roots are resolved
//! on each device separately, so the same snapshot restores correctly even if
//! the game lives in a different folder on another PC (or after an update).

use crate::adapters;
use crate::config::GameEntry;
use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct SaveSpec {
    pub roots: BTreeMap<String, String>,
    pub saves: Vec<String>,
    pub exclude: Vec<String>,
    pub wait_for: Vec<String>,
    pub quiesce_absent: Vec<String>,
    pub ludusavi_title: Option<String>,
}

/// Merge adapter rules with the user's overrides.
pub fn effective(game: &GameEntry) -> SaveSpec {
    let adapter = game.adapter.as_deref().and_then(adapters::find);
    let mut spec = SaveSpec::default();
    if let Some(a) = adapter {
        spec.roots = a.roots.clone();
        spec.saves = a.saves.clone();
        spec.exclude = a.exclude.clone();
        spec.wait_for = a.wait_for.clone();
        spec.quiesce_absent = a.quiesce.absent.clone();
    }
    for (k, v) in &game.roots {
        spec.roots.insert(k.clone(), v.clone());
    }
    if !game.saves.is_empty() {
        spec.saves = game.saves.clone();
    }
    spec.exclude.extend(game.exclude.iter().cloned());
    for w in &game.wait_for {
        if !spec.wait_for.iter().any(|x| x.eq_ignore_ascii_case(w)) {
            spec.wait_for.push(w.clone());
        }
    }
    spec.ludusavi_title = game.ludusavi_title.clone();
    spec
}

/// The game folder used for `{install}`.
pub fn install_dir(game: &GameEntry) -> Option<PathBuf> {
    game.install
        .clone()
        .or_else(|| game.exe.as_ref().and_then(|e| e.parent().map(Path::to_path_buf)))
}

/// Roots every game can use.
pub fn builtin_roots(install: Option<&Path>) -> BTreeMap<String, PathBuf> {
    let mut m = BTreeMap::new();
    if let Some(i) = install {
        m.insert("install".to_string(), i.to_path_buf());
    }
    if let Some(p) = dirs::home_dir() {
        m.insert("home".to_string(), p);
    }
    if let Some(p) = dirs::config_dir() {
        m.insert("appdata".to_string(), p);
    }
    if let Some(p) = dirs::data_local_dir() {
        m.insert("localappdata".to_string(), p);
    }
    if let Some(p) = dirs::document_dir() {
        m.insert("documents".to_string(), p);
    }
    m
}

/// Expand `{env:NAME}` and `{root}` tokens.
pub fn expand(template: &str, known: &BTreeMap<String, PathBuf>) -> Result<PathBuf> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let end = rest[start..]
            .find('}')
            .map(|e| start + e)
            .ok_or_else(|| anyhow!("路径模板缺少 }}：{template}"))?;
        let token = &rest[start + 1..end];
        if let Some(var) = token.strip_prefix("env:") {
            let v = std::env::var(var).map_err(|_| anyhow!("环境变量 {var} 不存在"))?;
            out.push_str(&v);
        } else if let Some(p) = resolve_root_name(token, known) {
            out.push_str(&p.to_string_lossy());
        } else {
            bail!("未知的根目录 {{{token}}}");
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(PathBuf::from(out.replace('/', "\\")))
}

/// Look up a root by name, including dynamic `drive-x` roots used for
/// absolute paths outside any known folder.
pub fn resolve_root_name(name: &str, known: &BTreeMap<String, PathBuf>) -> Option<PathBuf> {
    if let Some(p) = known.get(name) {
        return Some(p.clone());
    }
    let letter = name.strip_prefix("drive-")?;
    if letter.len() == 1 && letter.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some(PathBuf::from(format!("{}:\\", letter.to_uppercase())));
    }
    None
}

/// Resolve all roots for a game on this device.
pub fn resolve_roots(game: &GameEntry, spec: &SaveSpec) -> Result<BTreeMap<String, PathBuf>> {
    let mut roots = builtin_roots(install_dir(game).as_deref());
    for (name, tpl) in &spec.roots {
        let p = expand(tpl, &roots)?;
        roots.insert(name.clone(), p);
    }
    Ok(roots)
}

/// Split `{root}/rest` into `("root", "rest")` with forward slashes.
pub fn split_pattern(pattern: &str) -> Result<(String, String)> {
    let p = pattern.trim().replace('\\', "/");
    if !p.starts_with('{') {
        bail!("存档路径必须以 {{根目录}} 开头，例如 {{install}}/save/**：{pattern}");
    }
    let end = p
        .find('}')
        .ok_or_else(|| anyhow!("存档路径缺少 }}：{pattern}"))?;
    let root = p[1..end].to_string();
    let rest = p[end + 1..].trim_start_matches('/').to_string();
    Ok((root, rest))
}

/// Turn an absolute path into `{root}/rest` using the most specific root.
pub fn tokenize(abs: &Path, roots: &BTreeMap<String, PathBuf>) -> String {
    let abs_s = abs.to_string_lossy().replace('/', "\\");
    let abs_l = abs_s.to_lowercase();
    let mut best: Option<(&String, usize)> = None;
    for (name, p) in roots {
        let base = p.to_string_lossy().replace('/', "\\");
        let base_l = base.trim_end_matches('\\').to_lowercase();
        if base_l.is_empty() {
            continue;
        }
        if abs_l.starts_with(&(base_l.clone() + "\\")) && best.is_none_or(|(_, len)| base_l.len() > len) {
            best = Some((name, base_l.len()));
        }
    }
    if let Some((name, len)) = best {
        let rest = abs_s[len + 1..].replace('\\', "/");
        return format!("{name}/{rest}");
    }
    // Fall back to the drive letter.
    let mut chars = abs_s.chars();
    match (chars.next(), chars.next()) {
        (Some(d), Some(':')) if d.is_ascii_alphabetic() => {
            let rest = abs_s[2..].trim_start_matches('\\').replace('\\', "/");
            format!("drive-{}/{}", d.to_ascii_lowercase(), rest)
        }
        _ => format!("abs/{}", abs_s.replace('\\', "/")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits() {
        assert_eq!(
            split_pattern("{install}/save/**").unwrap(),
            ("install".into(), "save/**".into())
        );
        assert!(split_pattern("C:/x").is_err());
    }

    #[test]
    fn tokenizes() {
        let mut roots = BTreeMap::new();
        roots.insert("home".to_string(), PathBuf::from(r"C:\Users\me"));
        roots.insert("appdata".to_string(), PathBuf::from(r"C:\Users\me\AppData\Roaming"));
        assert_eq!(
            tokenize(Path::new(r"C:\Users\me\AppData\Roaming\Game\a.sav"), &roots),
            "appdata/Game/a.sav"
        );
        assert_eq!(tokenize(Path::new(r"D:\Saves\x.sav"), &roots), "drive-d/Saves/x.sav");
    }
}
