//! Native browser detection for Chromium-based browsers.
//!
//! Detects the user's default browser, resolves its executable path and
//! user-data directory (where cookies/logins live). Supports Chrome, Brave,
//! Edge, Arc, Vivaldi, Opera, and Chromium across macOS, Linux, and Windows.

use std::path::PathBuf;

/// Detected browser info.
#[derive(Debug, Clone)]
pub struct BrowserInfo {
    pub name: String,
    pub path: PathBuf,
    /// The browser's native user-data directory (where cookies/logins live).
    pub user_data_dir: Option<PathBuf>,
}

/// Known Chromium-based browser with platform-specific metadata.
struct BrowserCandidate {
    name: &'static str,
    /// Bundle ID (macOS) or desktop file (Linux) or ProgId (Windows).
    #[cfg(target_os = "macos")]
    bundle_id: &'static str,
    #[cfg(target_os = "linux")]
    desktop_file: &'static str,
    #[cfg(target_os = "windows")]
    prog_id: &'static str,
    /// Known executable paths per platform.
    paths: &'static [&'static str],
    /// Names for PATH lookup (e.g. "brave-browser", "google-chrome").
    which_names: &'static [&'static str],
    /// User data dir relative to platform config root.
    #[cfg(target_os = "macos")]
    profile_dir: Option<&'static str>,
    #[cfg(target_os = "linux")]
    profile_dir: Option<&'static str>,
    #[cfg(target_os = "windows")]
    profile_dir: Option<&'static str>,
}

/// Known Chromium-based browsers in preference order (most popular first).
fn known_browsers() -> Vec<BrowserCandidate> {
    vec![
        BrowserCandidate {
            name: "Google Chrome",
            #[cfg(target_os = "macos")]
            bundle_id: "com.google.chrome",
            #[cfg(target_os = "linux")]
            desktop_file: "google-chrome.desktop",
            #[cfg(target_os = "windows")]
            prog_id: "ChromeHTML",
            paths: if cfg!(target_os = "macos") {
                &["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"]
            } else if cfg!(target_os = "windows") {
                &[
                    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
                ]
            } else {
                &["/usr/bin/google-chrome-stable", "/usr/bin/google-chrome"]
            },
            which_names: &["google-chrome-stable", "google-chrome"],
            #[cfg(target_os = "macos")]
            profile_dir: Some("Google/Chrome"),
            #[cfg(target_os = "linux")]
            profile_dir: Some("google-chrome"),
            #[cfg(target_os = "windows")]
            profile_dir: Some(r"Google\Chrome\User Data"),
        },
        BrowserCandidate {
            name: "Brave",
            #[cfg(target_os = "macos")]
            bundle_id: "com.brave.Browser",
            #[cfg(target_os = "linux")]
            desktop_file: "brave-browser.desktop",
            #[cfg(target_os = "windows")]
            prog_id: "BraveHTML",
            paths: if cfg!(target_os = "macos") {
                &["/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"]
            } else if cfg!(target_os = "windows") {
                &[r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe"]
            } else {
                &[
                    "/usr/bin/brave-browser",
                    "/usr/bin/brave",
                    "/opt/brave.com/brave/brave",
                ]
            },
            which_names: &["brave-browser", "brave"],
            #[cfg(target_os = "macos")]
            profile_dir: Some("BraveSoftware/Brave-Browser"),
            #[cfg(target_os = "linux")]
            profile_dir: Some("BraveSoftware/Brave-Browser"),
            #[cfg(target_os = "windows")]
            profile_dir: Some(r"BraveSoftware\Brave-Browser\User Data"),
        },
        BrowserCandidate {
            name: "Microsoft Edge",
            #[cfg(target_os = "macos")]
            bundle_id: "com.microsoft.edgemac",
            #[cfg(target_os = "linux")]
            desktop_file: "microsoft-edge.desktop",
            #[cfg(target_os = "windows")]
            prog_id: "MSEdgeHTM",
            paths: if cfg!(target_os = "macos") {
                &["/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"]
            } else if cfg!(target_os = "windows") {
                &[
                    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
                    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                ]
            } else {
                &["/usr/bin/microsoft-edge", "/opt/microsoft/msedge/msedge"]
            },
            which_names: &["microsoft-edge", "msedge"],
            #[cfg(target_os = "macos")]
            profile_dir: Some("Microsoft Edge"),
            #[cfg(target_os = "linux")]
            profile_dir: Some("microsoft-edge"),
            #[cfg(target_os = "windows")]
            profile_dir: Some(r"Microsoft\Edge\User Data"),
        },
        BrowserCandidate {
            name: "Arc",
            #[cfg(target_os = "macos")]
            bundle_id: "company.thebrowser.Browser",
            #[cfg(target_os = "linux")]
            desktop_file: "",
            #[cfg(target_os = "windows")]
            prog_id: "",
            paths: if cfg!(target_os = "macos") {
                &["/Applications/Arc.app/Contents/MacOS/Arc"]
            } else {
                &[]
            },
            which_names: &[],
            #[cfg(target_os = "macos")]
            profile_dir: Some("Arc/User Data"),
            #[cfg(target_os = "linux")]
            profile_dir: None,
            #[cfg(target_os = "windows")]
            profile_dir: None,
        },
        BrowserCandidate {
            name: "Vivaldi",
            #[cfg(target_os = "macos")]
            bundle_id: "com.vivaldi.Vivaldi",
            #[cfg(target_os = "linux")]
            desktop_file: "vivaldi-stable.desktop",
            #[cfg(target_os = "windows")]
            prog_id: "VivaldiHTM",
            paths: if cfg!(target_os = "macos") {
                &["/Applications/Vivaldi.app/Contents/MacOS/Vivaldi"]
            } else if cfg!(target_os = "windows") {
                &[r"C:\Program Files\Vivaldi\Application\vivaldi.exe"]
            } else {
                &["/usr/bin/vivaldi", "/opt/vivaldi/vivaldi"]
            },
            which_names: &["vivaldi"],
            #[cfg(target_os = "macos")]
            profile_dir: Some("Vivaldi"),
            #[cfg(target_os = "linux")]
            profile_dir: Some("vivaldi"),
            #[cfg(target_os = "windows")]
            profile_dir: Some(r"Vivaldi\User Data"),
        },
        BrowserCandidate {
            name: "Opera",
            #[cfg(target_os = "macos")]
            bundle_id: "com.operasoftware.Opera",
            #[cfg(target_os = "linux")]
            desktop_file: "opera.desktop",
            #[cfg(target_os = "windows")]
            prog_id: "OperaStable",
            paths: if cfg!(target_os = "macos") {
                &["/Applications/Opera.app/Contents/MacOS/Opera"]
            } else if cfg!(target_os = "windows") {
                &[r"C:\Program Files\Opera\launcher.exe"]
            } else {
                &["/usr/bin/opera"]
            },
            which_names: &["opera"],
            #[cfg(target_os = "macos")]
            profile_dir: Some("com.operasoftware.Opera"),
            #[cfg(target_os = "linux")]
            profile_dir: Some("opera"),
            #[cfg(target_os = "windows")]
            profile_dir: Some(r"Opera Software\Opera Stable"),
        },
        BrowserCandidate {
            name: "Chromium",
            #[cfg(target_os = "macos")]
            bundle_id: "org.chromium.Chromium",
            #[cfg(target_os = "linux")]
            desktop_file: "chromium-browser.desktop",
            #[cfg(target_os = "windows")]
            prog_id: "ChromiumHTM",
            paths: if cfg!(target_os = "macos") {
                &["/Applications/Chromium.app/Contents/MacOS/Chromium"]
            } else if cfg!(target_os = "windows") {
                &[r"C:\Program Files\Chromium\Application\chrome.exe"]
            } else {
                &["/usr/bin/chromium-browser", "/usr/bin/chromium"]
            },
            which_names: &["chromium-browser", "chromium"],
            #[cfg(target_os = "macos")]
            profile_dir: Some("Chromium"),
            #[cfg(target_os = "linux")]
            profile_dir: Some("chromium"),
            #[cfg(target_os = "windows")]
            profile_dir: Some(r"Chromium\User Data"),
        },
    ]
}

/// Find the executable path for a browser candidate.
fn find_executable(candidate: &BrowserCandidate) -> Option<PathBuf> {
    for path in candidate.paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    for name in candidate.which_names {
        if let Ok(p) = which::which(name) {
            return Some(p);
        }
    }
    None
}

/// Resolve the browser's native user-data directory.
fn resolve_profile_dir(candidate: &BrowserCandidate) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = dirs::home_dir()?.join("Library/Application Support");
    #[cfg(target_os = "linux")]
    let base = dirs::config_dir()?;
    #[cfg(target_os = "windows")]
    let base = dirs::data_local_dir()?;

    let rel = candidate.profile_dir?;
    let dir = base.join(rel);
    if dir.exists() {
        Some(dir)
    } else {
        None
    }
}

/// Check if a profile directory is locked by a running browser instance.
pub fn is_profile_locked(profile_dir: &std::path::Path) -> bool {
    let lock = profile_dir.join("SingletonLock");
    if lock.exists() {
        return true;
    }
    let lock2 = profile_dir.join("Lock");
    if lock2.exists() {
        return true;
    }
    profile_dir.join("SingletonSocket").exists()
}

/// Detect the user's default browser (macOS).
///
/// Parses the LaunchServices plist to find which app handles `https`.
/// Each handler block contains `LSHandlerRoleAll` and `LSHandlerURLScheme`
/// in arbitrary order, so we collect both within each `{ ... }` block.
#[cfg(target_os = "macos")]
fn detect_default_browser_id() -> Option<String> {
    let output = std::process::Command::new("defaults")
        .args([
            "read",
            "com.apple.LaunchServices/com.apple.launchservices.secure",
            "LSHandlers",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);

    let mut role_all: Option<String> = None;
    let mut is_https = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Start of a new handler block — reset state
        if trimmed == "{" {
            role_all = None;
            is_https = false;
        }

        // Capture the RoleAll value (skip the nested PreferredVersions one)
        if trimmed.starts_with("LSHandlerRoleAll") && !trimmed.contains("-") {
            if let Some(eq) = trimmed.find('=') {
                let val = trimmed[eq + 1..]
                    .trim()
                    .trim_matches(';')
                    .trim()
                    .trim_matches('"');
                if !val.is_empty() && val != "-" {
                    role_all = Some(val.to_lowercase());
                }
            }
        }

        // Check if this block handles https
        if trimmed.contains("LSHandlerURLScheme") && trimmed.contains("https") {
            is_https = true;
        }

        // End of block — check if we found what we need
        if trimmed.starts_with("}") || trimmed.starts_with("},") {
            if is_https {
                if let Some(id) = role_all.take() {
                    return Some(id);
                }
            }
            role_all = None;
            is_https = false;
        }
    }
    None
}

/// Detect the user's default browser (Linux).
#[cfg(target_os = "linux")]
fn detect_default_browser_id() -> Option<String> {
    let output = std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Detect the user's default browser (Windows).
#[cfg(target_os = "windows")]
fn detect_default_browser_id() -> Option<String> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKEY_CURRENT_USER\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
            "/v",
            "ProgId",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.contains("ProgId") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(id) = parts.last() {
                return Some(id.to_lowercase());
            }
        }
    }
    None
}

/// Check if a browser candidate matches the detected default browser ID.
fn matches_default(candidate: &BrowserCandidate, default_id: &str) -> bool {
    let id = default_id.to_lowercase();
    #[cfg(target_os = "macos")]
    {
        candidate.bundle_id.to_lowercase() == id
    }
    #[cfg(target_os = "linux")]
    {
        candidate.desktop_file.to_lowercase() == id
    }
    #[cfg(target_os = "windows")]
    {
        candidate.prog_id.to_lowercase() == id
    }
}

/// Smart browser detection: finds the user's default browser, then falls back
/// to the first installed Chromium-based browser.
pub fn detect_browser() -> Option<BrowserInfo> {
    let browsers = known_browsers();

    // 1. Try the user's default browser first
    if let Some(default_id) = detect_default_browser_id() {
        tracing::debug!("Default browser identifier: {default_id}");
        for candidate in &browsers {
            if matches_default(candidate, &default_id) {
                if let Some(path) = find_executable(candidate) {
                    tracing::info!(
                        "Default browser detected: {} ({})",
                        candidate.name,
                        default_id
                    );
                    return Some(BrowserInfo {
                        name: candidate.name.to_string(),
                        path,
                        user_data_dir: resolve_profile_dir(candidate),
                    });
                }
            }
        }
        tracing::debug!("Default browser '{default_id}' is not Chromium-based or not found");
    }

    // 2. Fall back to first installed Chromium browser
    for candidate in &browsers {
        if let Some(path) = find_executable(candidate) {
            tracing::info!("Found Chromium browser: {}", candidate.name);
            return Some(BrowserInfo {
                name: candidate.name.to_string(),
                path,
                user_data_dir: resolve_profile_dir(candidate),
            });
        }
    }

    tracing::warn!("No Chromium-based browser found on system");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_browsers_not_empty() {
        let browsers = known_browsers();
        assert!(browsers.len() >= 7, "Expected at least 7 known browsers");
    }

    #[test]
    fn test_is_profile_locked_nonexistent() {
        let dir = PathBuf::from("/tmp/nonexistent-browser-profile-test-opencli");
        assert!(!is_profile_locked(&dir));
    }

    #[test]
    fn test_detect_default_browser_id_no_panic() {
        // May return None on CI — just ensure no panic
        let _ = detect_default_browser_id();
    }

    #[test]
    fn test_detect_browser_no_panic() {
        // May return None on CI — just ensure no panic
        let _ = detect_browser();
    }

    #[test]
    fn test_find_executable_nonexistent() {
        let candidate = BrowserCandidate {
            name: "FakeBrowser",
            #[cfg(target_os = "macos")]
            bundle_id: "com.fake.browser",
            #[cfg(target_os = "linux")]
            desktop_file: "fake.desktop",
            #[cfg(target_os = "windows")]
            prog_id: "FakeHTML",
            paths: &["/nonexistent/path/to/fake-browser"],
            which_names: &["fake-browser-that-does-not-exist"],
            #[cfg(target_os = "macos")]
            profile_dir: None,
            #[cfg(target_os = "linux")]
            profile_dir: None,
            #[cfg(target_os = "windows")]
            profile_dir: None,
        };
        assert!(find_executable(&candidate).is_none());
    }

    #[test]
    fn test_resolve_profile_dir_nonexistent() {
        let candidate = BrowserCandidate {
            name: "FakeBrowser",
            #[cfg(target_os = "macos")]
            bundle_id: "",
            #[cfg(target_os = "linux")]
            desktop_file: "",
            #[cfg(target_os = "windows")]
            prog_id: "",
            paths: &[],
            which_names: &[],
            #[cfg(target_os = "macos")]
            profile_dir: Some("NonexistentBrowser/Data"),
            #[cfg(target_os = "linux")]
            profile_dir: Some("nonexistent-browser"),
            #[cfg(target_os = "windows")]
            profile_dir: Some(r"NonexistentBrowser\Data"),
        };
        assert!(resolve_profile_dir(&candidate).is_none());
    }

    #[test]
    fn test_matches_default_case_insensitive() {
        let candidate = BrowserCandidate {
            name: "Test",
            #[cfg(target_os = "macos")]
            bundle_id: "com.google.Chrome",
            #[cfg(target_os = "linux")]
            desktop_file: "Google-Chrome.desktop",
            #[cfg(target_os = "windows")]
            prog_id: "ChromeHTML",
            paths: &[],
            which_names: &[],
            #[cfg(target_os = "macos")]
            profile_dir: None,
            #[cfg(target_os = "linux")]
            profile_dir: None,
            #[cfg(target_os = "windows")]
            profile_dir: None,
        };
        #[cfg(target_os = "macos")]
        assert!(matches_default(&candidate, "com.google.chrome"));
        #[cfg(target_os = "linux")]
        assert!(matches_default(&candidate, "google-chrome.desktop"));
        #[cfg(target_os = "windows")]
        assert!(matches_default(&candidate, "chromehtml"));
    }
}
