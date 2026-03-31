//! Simple i18n: detect OS language, show Chinese for zh, English for others.

use std::sync::OnceLock;

static IS_CHINESE: OnceLock<bool> = OnceLock::new();

/// Detect if the OS language is Chinese.
fn detect_chinese() -> bool {
    // Check LANG, LC_ALL, LANGUAGE env vars
    for var in &["LANG", "LC_ALL", "LANGUAGE"] {
        if let Ok(val) = std::env::var(var) {
            let lower = val.to_lowercase();
            if lower.starts_with("zh") {
                return true;
            }
        }
    }

    // macOS: check AppleLocale
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLocale"])
            .output()
        {
            let locale = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if locale.starts_with("zh") {
                return true;
            }
        }
    }

    // Windows: check via PowerShell Get-Culture
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "(Get-Culture).Name"])
            .output()
        {
            let culture = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if culture.starts_with("zh") {
                return true;
            }
        }
    }

    false
}

/// Returns true if the OS language is Chinese.
pub fn is_chinese() -> bool {
    *IS_CHINESE.get_or_init(detect_chinese)
}

/// Select text based on OS language: Chinese or English.
pub fn t<'a>(zh: &'a str, en: &'a str) -> &'a str {
    if is_chinese() { zh } else { en }
}
