use async_trait::async_trait;
use opencli_rs_core::{
    AutoScrollOptions, CliError, Cookie, CookieOptions, GotoOptions, IPage, InterceptedRequest,
    NetworkRequest, ScreenshotOptions, ScrollDirection, SnapshotOptions, TabInfo, WaitOptions,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::daemon_client::DaemonClient;
use crate::dom_helpers;
use crate::types::DaemonCommand;

/// A page backed by the Daemon + Chrome Extension bridge.
pub struct DaemonPage {
    client: Arc<DaemonClient>,
    workspace: String,
    tab_id: RwLock<Option<u64>>,
}

impl DaemonPage {
    pub fn new(client: Arc<DaemonClient>, workspace: impl Into<String>) -> Self {
        Self {
            client,
            workspace: workspace.into(),
            tab_id: RwLock::new(None),
        }
    }

    /// Build a command with workspace and optional tab_id pre-filled.
    async fn cmd(&self, action: &str) -> DaemonCommand {
        let mut c = DaemonCommand::new(action).with_workspace(self.workspace.clone());
        if let Some(tid) = *self.tab_id.read().await {
            c = c.with_tab_id(tid);
        }
        c
    }

    /// Send a command via the daemon client.
    async fn send(&self, cmd: DaemonCommand) -> Result<Value, CliError> {
        self.client.send_command(cmd).await
    }

    /// Evaluate JS on the current page via the daemon.
    async fn eval_js(&self, code: &str) -> Result<Value, CliError> {
        let cmd = self.cmd("exec").await.with_code(code);
        self.send(cmd).await
    }
}

#[async_trait]
impl IPage for DaemonPage {
    async fn goto(&self, url: &str, _options: Option<GotoOptions>) -> Result<(), CliError> {
        let cmd = self.cmd("navigate").await.with_url(url);
        self.send(cmd).await?;
        // The Chrome extension's handleNavigate already waits for the page to
        // fully load (URL change + status=complete, up to 15s). No additional
        // DOM stability check is needed here.
        Ok(())
    }

    async fn url(&self) -> Result<String, CliError> {
        let val = self.eval_js("window.location.href").await?;
        Ok(val.as_str().unwrap_or("").to_string())
    }

    async fn title(&self) -> Result<String, CliError> {
        let val = self.eval_js("document.title").await?;
        Ok(val.as_str().unwrap_or("").to_string())
    }

    async fn content(&self) -> Result<String, CliError> {
        let val = self.eval_js("document.documentElement.outerHTML").await?;
        Ok(val.as_str().unwrap_or("").to_string())
    }

    async fn evaluate(&self, expression: &str) -> Result<Value, CliError> {
        self.eval_js(expression).await
    }

    async fn wait_for_selector(
        &self,
        selector: &str,
        options: Option<WaitOptions>,
    ) -> Result<(), CliError> {
        let opts = options.unwrap_or_default();
        let timeout_ms = opts.timeout_ms.unwrap_or(30_000);
        let visible = opts.visible.unwrap_or(false);
        let js = dom_helpers::wait_for_selector_js(selector, timeout_ms, visible);
        self.eval_js(&js).await?;
        Ok(())
    }

    async fn wait_for_navigation(&self, _options: Option<WaitOptions>) -> Result<(), CliError> {
        let js = dom_helpers::wait_for_dom_stable_js();
        self.eval_js(&js).await?;
        Ok(())
    }

    async fn wait_for_timeout(&self, ms: u64) -> Result<(), CliError> {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        Ok(())
    }

    async fn click(&self, selector: &str) -> Result<(), CliError> {
        let js = dom_helpers::click_js(selector);
        self.eval_js(&js).await?;
        Ok(())
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<(), CliError> {
        let js = dom_helpers::type_text_js(selector, text);
        self.eval_js(&js).await?;
        Ok(())
    }

    async fn cookies(&self, _options: Option<CookieOptions>) -> Result<Vec<Cookie>, CliError> {
        let cmd = self.cmd("cookies").await;
        let val = self.send(cmd).await?;
        let cookies: Vec<Cookie> = serde_json::from_value(val).unwrap_or_default();
        Ok(cookies)
    }

    async fn set_cookies(&self, cookies: Vec<Cookie>) -> Result<(), CliError> {
        let js = format!(
            "(() => {{ {} return true; }})()",
            cookies
                .iter()
                .map(|c| format!(
                    "document.cookie = '{}={}; path={}';",
                    c.name,
                    c.value,
                    c.path.as_deref().unwrap_or("/")
                ))
                .collect::<Vec<_>>()
                .join(" ")
        );
        self.eval_js(&js).await?;
        Ok(())
    }

    async fn screenshot(&self, _options: Option<ScreenshotOptions>) -> Result<Vec<u8>, CliError> {
        let cmd = self.cmd("screenshot").await;
        let val = self.send(cmd).await?;
        // Expect base64-encoded data from the daemon
        if let Some(b64) = val.as_str() {
            Ok(base64_decode_simple(b64))
        } else {
            Ok(Vec::new())
        }
    }

    async fn snapshot(&self, options: Option<SnapshotOptions>) -> Result<Value, CliError> {
        let opts = options.unwrap_or_default();
        let js = dom_helpers::snapshot_js(opts.selector.as_deref(), opts.include_hidden);
        self.eval_js(&js).await
    }

    async fn auto_scroll(&self, options: Option<AutoScrollOptions>) -> Result<(), CliError> {
        let opts = options.unwrap_or_default();
        let max = opts.max_scrolls.unwrap_or(20);
        let delay = opts.delay_ms.unwrap_or(300);
        let js = match opts.direction {
            ScrollDirection::Up => {
                format!(
                    r#"(async () => {{
  let scrolls = 0;
  while (scrolls < {max}) {{
    window.scrollBy(0, -window.innerHeight);
    await new Promise(r => setTimeout(r, {delay}));
    if (window.scrollY === 0) break;
    scrolls++;
  }}
  return scrolls;
}})()"#
                )
            }
            ScrollDirection::Down => dom_helpers::auto_scroll_js(max, delay),
        };
        self.eval_js(&js).await?;
        Ok(())
    }

    async fn tabs(&self) -> Result<Vec<TabInfo>, CliError> {
        let cmd = self.cmd("tabs").await;
        let val = self.send(cmd).await?;
        let tabs: Vec<TabInfo> = serde_json::from_value(val).unwrap_or_default();
        Ok(tabs)
    }

    async fn switch_tab(&self, tab_id: &str) -> Result<(), CliError> {
        let tid: u64 = tab_id
            .parse()
            .map_err(|_| CliError::argument(format!("Invalid tab id: {tab_id}")))?;
        *self.tab_id.write().await = Some(tid);
        let mut cmd = self.cmd("tabs").await;
        cmd.url = Some("switch".to_string());
        cmd.tab_id = Some(tid);
        self.send(cmd).await?;
        Ok(())
    }

    async fn close(&self) -> Result<(), CliError> {
        let cmd = self.cmd("close-window").await;
        self.send(cmd).await?;
        Ok(())
    }

    async fn intercept_requests(&self, url_pattern: &str) -> Result<(), CliError> {
        let js = dom_helpers::install_interceptor_js(url_pattern);
        self.eval_js(&js).await?;
        Ok(())
    }

    async fn get_intercepted_requests(&self) -> Result<Vec<InterceptedRequest>, CliError> {
        let js = dom_helpers::get_intercepted_requests_js();
        let val = self.eval_js(&js).await?;
        let reqs: Vec<InterceptedRequest> = serde_json::from_value(val).unwrap_or_default();
        Ok(reqs)
    }

    async fn get_network_requests(&self) -> Result<Vec<NetworkRequest>, CliError> {
        let js = dom_helpers::network_requests_js();
        let val = self.eval_js(&js).await?;
        let reqs: Vec<NetworkRequest> = serde_json::from_value(val).unwrap_or_default();
        Ok(reqs)
    }
}

/// Simple base64 decoder (avoiding an extra dependency). Public for reuse by cdp module.
pub(crate) fn base64_decode_simple(input: &str) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    }
    let _ = TABLE; // suppress unused warning

    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'=' && b != b'\n' && b != b'\r')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let n = chunk.len();
        if n < 2 {
            break;
        }
        let a = val(chunk[0]);
        let b = val(chunk[1]);
        out.push((a << 2) | (b >> 4));
        if n > 2 {
            let c = val(chunk[2]);
            out.push((b << 4) | (c >> 2));
            if n > 3 {
                let d = val(chunk[3]);
                out.push((c << 6) | d);
            }
        }
    }
    out
}
