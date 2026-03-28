use opencli_rs_core::CliError;
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, warn};

use crate::types::{DaemonCommand, DaemonResult};

/// HTTP client that communicates with the Daemon server.
pub struct DaemonClient {
    base_url: String,
    client: reqwest::Client,
}

/// Retry delays for exponential backoff.
const RETRY_DELAYS_MS: [u64; 4] = [200, 500, 1000, 2000];

impl DaemonClient {
    /// Create a new client pointing at the given port on localhost.
    pub fn new(port: u16) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            client,
        }
    }

    /// Send a command to the daemon and return the result data.
    ///
    /// Retries up to 4 times with exponential backoff on transient failures.
    pub async fn send_command(&self, cmd: DaemonCommand) -> Result<Value, CliError> {
        let url = format!("{}/command", self.base_url);
        let mut last_err: Option<String> = None;

        for (attempt, &delay_ms) in RETRY_DELAYS_MS.iter().enumerate() {
            debug!(attempt = attempt + 1, action = %cmd.action, "sending daemon command");

            let result = self
                .client
                .post(&url)
                .header("X-OpenCLI", "1")
                .json(&cmd)
                .send()
                .await;

            match result {
                Ok(resp) => {
                    let status = resp.status();

                    if status.is_success() {
                        let daemon_result: DaemonResult = resp.json().await.map_err(|e| {
                            CliError::browser_connect(format!(
                                "Failed to parse daemon response: {e}"
                            ))
                        })?;
                        if daemon_result.ok {
                            return Ok(daemon_result.data.unwrap_or(Value::Null));
                        } else {
                            let err_msg = daemon_result
                                .error
                                .unwrap_or_else(|| "Unknown daemon error".into());
                            return Err(CliError::command_execution(format!(
                                "Daemon command failed: {err_msg}"
                            )));
                        }
                    }

                    // 4xx errors are client/business errors — don't retry
                    if status.is_client_error() {
                        let body = resp.text().await.unwrap_or_default();
                        return Err(CliError::command_execution(format!(
                            "Command error (HTTP {status}): {body}"
                        )));
                    }

                    // 5xx / other errors — retryable
                    let body = resp.text().await.unwrap_or_default();
                    last_err = Some(format!("HTTP {status}: {body}"));
                }
                Err(e) => {
                    // Network error — retryable
                    last_err = Some(format!("Request error: {e}"));
                }
            }

            if attempt < RETRY_DELAYS_MS.len() - 1 {
                warn!(
                    attempt = attempt + 1,
                    error = last_err.as_deref().unwrap_or("unknown"),
                    "retrying daemon command"
                );
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }

        Err(CliError::browser_connect(format!(
            "Failed to send command after {} attempts: {}",
            RETRY_DELAYS_MS.len(),
            last_err.unwrap_or_else(|| "unknown error".into())
        )))
    }

    /// Check if something is listening on the daemon port.
    /// Tries our health endpoint first, then falls back to checking with X-OpenCLI header
    /// (needed for the original opencli daemon which requires it on all requests).
    pub async fn is_running(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        // Try without header first (our daemon doesn't require it)
        if let Ok(resp) = self.client.get(&url).send().await {
            if resp.status().is_success() {
                return true;
            }
            // Got a response (even 403) means something is listening
            if resp.status().as_u16() == 403 {
                return true;
            }
        }
        false
    }

    /// Check if the Chrome extension is connected to the daemon.
    /// Compatible with both opencli-rs (`extension` field) and original opencli (`extensionConnected` field).
    pub async fn is_extension_connected(&self) -> bool {
        let url = format!("{}/status", self.base_url);
        // Original opencli requires X-OpenCLI header on all requests
        match self.client.get(&url).header("X-OpenCLI", "1").send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json) = resp.json::<Value>().await {
                    // Our format: {"extension": bool}
                    // Original format: {"extensionConnected": bool}
                    json.get("extension")
                        .or_else(|| json.get("extensionConnected"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_client_construction() {
        let client = DaemonClient::new(19825);
        assert_eq!(client.base_url, "http://127.0.0.1:19825");
    }

    #[tokio::test]
    async fn test_is_running_when_no_server() {
        // Pick a port that's almost certainly not in use
        let client = DaemonClient::new(19999);
        assert!(!client.is_running().await);
    }

    #[tokio::test]
    async fn test_is_extension_connected_when_no_server() {
        let client = DaemonClient::new(19999);
        assert!(!client.is_extension_connected().await);
    }
}
