use opencli_rs_core::{CliError, IPage};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::daemon_client::DaemonClient;
use crate::page::DaemonPage;

const DEFAULT_PORT: u16 = 19825;
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(200);
const EXTENSION_INITIAL_WAIT: Duration = Duration::from_secs(5);
const EXTENSION_REMAINING_WAIT: Duration = Duration::from_secs(25);
const EXTENSION_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// High-level bridge that manages the Daemon process and provides IPage instances.
/// The daemon runs as a detached background process with its own idle-shutdown lifecycle.
pub struct BrowserBridge {
    port: u16,
}

impl BrowserBridge {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// Create a bridge using the default port.
    pub fn default_port() -> Self {
        Self::new(DEFAULT_PORT)
    }

    /// Connect to the daemon, starting it if necessary, and return a page.
    pub async fn connect(&mut self) -> Result<Arc<dyn IPage>, CliError> {
        let client = Arc::new(DaemonClient::new(self.port));

        // Step 1: Check Chrome is running
        if !is_chrome_running() {
            return Err(CliError::BrowserConnect {
                message: "Chrome is not running".into(),
                suggestions: vec![
                    "Please open Google Chrome with the OpenCLI extension installed".into(),
                    "The extension connects to the daemon automatically when Chrome is open".into(),
                ],
                source: None,
            });
        }

        // Step 2: Ensure daemon is running
        if client.is_running().await {
            debug!(port = self.port, "daemon already running, reusing");
        } else {
            info!(port = self.port, "daemon not running, spawning");
            self.spawn_daemon().await?;
            self.wait_for_ready(&client).await?;
        }

        // Step 3: Wait up to 5s for extension to connect
        if self
            .poll_extension(&client, EXTENSION_INITIAL_WAIT, false)
            .await
        {
            let page = DaemonPage::new(client, "default");
            return Ok(Arc::new(page));
        }

        // Step 4: Extension not connected — try to wake up Chrome
        info!("Extension not connected after 5s, attempting to wake up Chrome");
        eprintln!("Waking up Chrome extension...");
        wake_chrome();

        // Step 5: Wait remaining 25s with progress
        if self
            .poll_extension(&client, EXTENSION_REMAINING_WAIT, true)
            .await
        {
            let page = DaemonPage::new(client, "default");
            return Ok(Arc::new(page));
        }

        warn!("Chrome extension is not connected to the daemon");
        Err(CliError::BrowserConnect {
            message: "Chrome extension not connected".into(),
            suggestions: vec![
                "Make sure the OpenCLI Chrome extension is installed and enabled".into(),
                "Try opening a new Chrome window manually".into(),
                format!("The daemon is listening on port {}", self.port),
            ],
            source: None,
        })
    }

    /// Spawn the daemon as a child process using --daemon flag on the current binary.
    async fn spawn_daemon(&mut self) -> Result<(), CliError> {
        let exe = std::env::current_exe().map_err(|e| {
            CliError::browser_connect(format!("Cannot determine current executable: {e}"))
        })?;

        let child = tokio::process::Command::new(exe)
            .arg("--daemon")
            .arg("--port")
            .arg(self.port.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| CliError::browser_connect(format!("Failed to spawn daemon: {e}")))?;

        info!(port = self.port, pid = ?child.id(), "daemon process spawned (detached)");
        std::mem::forget(child);
        Ok(())
    }

    /// Poll for extension connection within the given duration.
    /// Returns true if connected, false if timed out.
    async fn poll_extension(
        &self,
        client: &DaemonClient,
        timeout: Duration,
        show_progress: bool,
    ) -> bool {
        let start = tokio::time::Instant::now();
        let deadline = start + timeout;
        let mut printed = false;

        while tokio::time::Instant::now() < deadline {
            if client.is_extension_connected().await {
                if printed {
                    eprintln!();
                }
                info!("Chrome extension connected");
                return true;
            }

            if show_progress {
                let elapsed = start.elapsed().as_secs();
                if elapsed >= 1 && !printed {
                    eprint!("Waiting for Chrome extension to connect");
                    printed = true;
                } else if printed && elapsed.is_multiple_of(3) {
                    eprint!(".");
                }
            }

            tokio::time::sleep(EXTENSION_POLL_INTERVAL).await;
        }

        if printed {
            eprintln!();
        }
        false
    }

    /// Wait for the daemon to become ready by polling /health.
    async fn wait_for_ready(&self, client: &DaemonClient) -> Result<(), CliError> {
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;

        while tokio::time::Instant::now() < deadline {
            if client.is_running().await {
                info!("daemon is ready");
                return Ok(());
            }
            tokio::time::sleep(READY_POLL_INTERVAL).await;
        }

        Err(CliError::timeout(format!(
            "Daemon did not become ready within {}s",
            READY_TIMEOUT.as_secs()
        )))
    }
}

/// Check if Chrome/Chromium is running as a process.
fn is_chrome_running() -> bool {
    if cfg!(target_os = "macos") {
        // macOS: check for "Google Chrome" process
        std::process::Command::new("pgrep")
            .args(["-x", "Google Chrome"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else if cfg!(target_os = "windows") {
        // Windows: check for chrome.exe
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq chrome.exe", "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("chrome.exe"))
            .unwrap_or(false)
    } else {
        // Linux: check for chrome or chromium
        std::process::Command::new("pgrep")
            .args(["-x", "chrome|chromium"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Try to wake up Chrome by opening a window.
/// When Chrome is running but has no windows, the extension Service Worker is suspended.
/// Opening a window activates the Service Worker, which then reconnects to the daemon.
fn wake_chrome() {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .args(["-a", "Google Chrome", "about:blank"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "chrome", "about:blank"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else {
        // Linux: try common Chrome executables
        std::process::Command::new("xdg-open")
            .arg("about:blank")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };

    match result {
        Ok(_) => debug!("Opened Chrome window to wake extension"),
        Err(e) => debug!("Failed to open Chrome window: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_construction() {
        let bridge = BrowserBridge::new(19825);
        assert_eq!(bridge.port, 19825);
    }

    #[test]
    fn test_bridge_default_port() {
        let bridge = BrowserBridge::default_port();
        assert_eq!(bridge.port, DEFAULT_PORT);
    }
}
