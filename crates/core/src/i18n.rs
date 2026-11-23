//! Interface language for texts made in Rust: errors shown in the app, the
//! Steam wrapper's dialogs, the tray menu and the CLI.
//!
//! The language is the user's choice in Settings (`AppConfig::language`:
//! "system", "zh-CN" or "en"); "system" follows the Windows display
//! language. Each program calls [`set`] once at start (the app again when
//! the user changes it).
//!
//! Texts are written in both languages where they are used:
//!
//! ```ignore
//! bail!(tr!("找不到「{name}」", "Can't find {name}"));
//! let text = tr!("已上传 {} 个文件", "Uploaded {} files", n);
//! ```

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    ZhCn,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::ZhCn => "zh-CN",
            Lang::En => "en",
        }
    }
}

/// 0 = follow the system, 1 = Chinese, 2 = English.
static CHOICE: AtomicU8 = AtomicU8::new(0);

/// Apply the user's choice ("system", "zh-CN" or "en").
pub fn set(pref: &str) {
    let v = match pref {
        "zh-CN" => 1,
        "en" => 2,
        _ => 0,
    };
    CHOICE.store(v, Ordering::Relaxed);
}

pub fn current() -> Lang {
    match CHOICE.load(Ordering::Relaxed) {
        1 => Lang::ZhCn,
        2 => Lang::En,
        _ => system(),
    }
}

pub fn is_en() -> bool {
    current() == Lang::En
}

/// What "system" means: `LUDONIMBUS_LANG` if set (testing), otherwise the
/// Windows display language (Chinese if its primary language is Chinese,
/// otherwise English).
pub fn system() -> Lang {
    match std::env::var("LUDONIMBUS_LANG").as_deref() {
        Ok("zh-CN") => Lang::ZhCn,
        Ok("en") => Lang::En,
        _ => os_lang(),
    }
}

#[cfg(windows)]
fn os_lang() -> Lang {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    // LANG_CHINESE is primary language id 0x04 (low 10 bits of the LANGID).
    let id = unsafe { GetUserDefaultUILanguage() };
    if id & 0x3ff == 0x04 {
        Lang::ZhCn
    } else {
        Lang::En
    }
}

#[cfg(not(windows))]
fn os_lang() -> Lang {
    let l = std::env::var("LC_ALL").or_else(|_| std::env::var("LANG")).unwrap_or_default();
    if l.to_lowercase().starts_with("zh") {
        Lang::ZhCn
    } else {
        Lang::En
    }
}

/// Text in the current language: `tr!("中文", "English", args…)`, formatted
/// like `format!` (inline `{name}` captures work too). Returns a `String`.
#[macro_export]
macro_rules! tr {
    ($zh:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::is_en() {
            format!($en $(, $arg)*)
        } else {
            format!($zh $(, $arg)*)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_language_and_formats() {
        let name = "Zelda";
        set("en");
        assert_eq!(tr!("找不到「{name}」", "Can't find {name}"), "Can't find Zelda");
        assert_eq!(tr!("{} 个文件", "{} files", 3), "3 files");
        set("zh-CN");
        assert_eq!(tr!("找不到「{name}」", "Can't find {name}"), "找不到「Zelda」");
        set("system");
    }
}
