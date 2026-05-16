use async_trait::async_trait;
use autocli_core::{
    AutoScrollOptions, CliError, Cookie, CookieOptions, GotoOptions, IPage, InterceptedRequest,
    NetworkRequest, ScreenshotOptions, SnapshotOptions, TabInfo, WaitOptions,
};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, error};

use crate::dom_helpers;

type WsSink = futures::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

/// Direct Chrome DevTools Protocol page client via WebSocket.
///
/// Used when `AUTOCLI_CDP_ENDPOINT` is set (e.g., connecting to a headless Chrome instance).
pub struct CdpPage {
    ws_write: Mutex<WsSink>,
    pending: Arc<RwLock<HashMap<u64, oneshot::Sender<Value>>>>,
    cmd_id: AtomicU64,
}

impl CdpPage {
    /// Connect to a CDP WebSocket endpoint (e.g., `ws://127.0.0.1:9222/devtools/page/...`).
    pub async fn connect(endpoint: &str) -> Result<Self, CliError> {
        let (ws_stream, _) = connect_async(endpoint).await.map_err(|e| {
            CliError::browser_connect(format!("Failed to connect to CDP endpoint: {e}"))
        })?;

        let (write, mut read) = ws_stream.split();
        let pending: Arc<RwLock<HashMap<u64, oneshot::Sender<Value>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Spawn reader task
        let reader_pending = pending.clone();
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(json) = serde_json::from_str::<Value>(&text) {
                            if let Some(id) = json.get("id").and_then(|v| v.as_u64()) {
                                if let Some(tx) = reader_pending.write().await.remove(&id) {
                                    let _ = tx.send(json);
                                }
                            } else {
                                debug!(event = %text.chars().take(100).collect::<String>(), "CDP event");
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        debug!("CDP WebSocket closed");
                        break;
                    }
                    Err(e) => {
                        error!("CDP WebSocket read error: {e}");
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(Self {
            ws_write: Mutex::new(write),
            pending,
            cmd_id: AtomicU64::new(1),
        })
    }

    /// Send a CDP command and await the response.
    async fn send_cdp(&self, method: &str, params: Value) -> Result<Value, CliError> {
        let id = self.cmd_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let (tx, rx) = oneshot::channel();
        self.pending.write().await.insert(id, tx);

        {
            let mut ws = self.ws_write.lock().await;
            ws.send(Message::Text(msg.to_string().into()))
                .await
                .map_err(|e| CliError::browser_connect(format!("CDP send error: {e}")))?;
        }

        match tokio::time::timeout(Duration::from_secs(60), rx).await {
            Ok(Ok(result)) => {
                if let Some(err) = result.get("error") {
                    Err(CliError::command_execution(format!(
                        "CDP error: {}",
                        err.get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown")
                    )))
                } else {
                    Ok(result.get("result").cloned().unwrap_or(Value::Null))
                }
            }
            Ok(Err(_)) => Err(CliError::browser_connect("CDP response channel closed")),
            Err(_) => {
                self.pending.write().await.remove(&id);
                Err(CliError::timeout("CDP command timed out (60s)"))
            }
        }
    }

    /// Evaluate JS via Runtime.evaluate.
    async fn evaluate_js(&self, expression: &str, await_promise: bool) -> Result<Value, CliError> {
        let result = self
            .send_cdp(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": await_promise,
                }),
            )
            .await?;

        if let Some(exception) = result.get("exceptionDetails") {
            let text = exception
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("JS exception");
            return Err(CliError::command_execution(format!("JS error: {text}")));
        }

        Ok(result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }
}

#[async_trait]
impl IPage for CdpPage {
    async fn goto(&self, url: &str, _options: Option<GotoOptions>) -> Result<(), CliError> {
        self.send_cdp("Page.navigate", json!({ "url": url }))
            .await?;
        // Wait for load event
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    }

    async fn url(&self) -> Result<String, CliError> {
        let val = self.evaluate_js("window.location.href", false).await?;
        Ok(val.as_str().unwrap_or("").to_string())
    }

    async fn title(&self) -> Result<String, CliError> {
        let val = self.evaluate_js("document.title", false).await?;
        Ok(val.as_str().unwrap_or("").to_string())
    }

    async fn content(&self) -> Result<String, CliError> {
        let val = self
            .evaluate_js("document.documentElement.outerHTML", false)
            .await?;
        Ok(val.as_str().unwrap_or("").to_string())
    }

    async fn evaluate(&self, expression: &str) -> Result<Value, CliError> {
        // Auto-detect async by checking for common async patterns
        let is_async = expression.contains("await ") || expression.starts_with("(async");
        self.evaluate_js(expression, is_async).await
    }

    async fn wait_for_selector(
        &self,
        selector: &str,
        options: Option<WaitOptions>,
    ) -> Result<(), CliError> {
        let opts = options.unwrap_or_default();
        let timeout = opts.timeout_ms.unwrap_or(30_000);
        let visible = opts.visible.unwrap_or(false);
        let js = dom_helpers::wait_for_selector_js(selector, timeout, visible);
        self.evaluate_js(&js, true).await?;
        Ok(())
    }

    async fn wait_for_navigation(&self, _options: Option<WaitOptions>) -> Result<(), CliError> {
        let js = dom_helpers::wait_for_dom_stable_js();
        self.evaluate_js(&js, true).await?;
        Ok(())
    }

    async fn wait_for_timeout(&self, ms: u64) -> Result<(), CliError> {
        tokio::time::sleep(Duration::from_millis(ms)).await;
        Ok(())
    }

    async fn click(&self, selector: &str) -> Result<(), CliError> {
        let js = dom_helpers::click_js(selector);
        self.evaluate_js(&js, false).await?;
        Ok(())
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<(), CliError> {
        let js = dom_helpers::type_text_js(selector, text);
        self.evaluate_js(&js, false).await?;
        Ok(())
    }

    async fn cookies(&self, _options: Option<CookieOptions>) -> Result<Vec<Cookie>, CliError> {
        let result = self.send_cdp("Network.getCookies", json!({})).await?;
        let cookies_val = result.get("cookies").cloned().unwrap_or(json!([]));
        let cookies: Vec<Cookie> = serde_json::from_value(cookies_val).unwrap_or_default();
        Ok(cookies)
    }

    async fn set_cookies(&self, cookies: Vec<Cookie>) -> Result<(), CliError> {
        for cookie in &cookies {
            self.send_cdp(
                "Network.setCookie",
                json!({
                    "name": cookie.name,
                    "value": cookie.value,
                    "domain": cookie.domain,
                    "path": cookie.path.as_deref().unwrap_or("/"),
                }),
            )
            .await?;
        }
        Ok(())
    }

    async fn screenshot(&self, _options: Option<ScreenshotOptions>) -> Result<Vec<u8>, CliError> {
        let result = self
            .send_cdp("Page.captureScreenshot", json!({ "format": "png" }))
            .await?;
        if let Some(data) = result.get("data").and_then(|d| d.as_str()) {
            Ok(crate::page::base64_decode_simple(data))
        } else {
            Ok(Vec::new())
        }
    }

    async fn snapshot(&self, options: Option<SnapshotOptions>) -> Result<Value, CliError> {
        let opts = options.unwrap_or_default();
        let js = dom_helpers::snapshot_js(opts.selector.as_deref(), opts.include_hidden);
        self.evaluate_js(&js, false).await
    }

    async fn auto_scroll(&self, options: Option<AutoScrollOptions>) -> Result<(), CliError> {
        let opts = options.unwrap_or_default();
        let max = opts.max_scrolls.unwrap_or(20);
        let delay = opts.delay_ms.unwrap_or(300);
        let js = dom_helpers::auto_scroll_js(max, delay);
        self.evaluate_js(&js, true).await?;
        Ok(())
    }

    async fn tabs(&self) -> Result<Vec<TabInfo>, CliError> {
        let result = self.send_cdp("Target.getTargets", json!({})).await?;
        let targets = result.get("targetInfos").cloned().unwrap_or(json!([]));
        let mut tabs = Vec::new();
        if let Some(arr) = targets.as_array() {
            for t in arr {
                if t.get("type").and_then(|v| v.as_str()) == Some("page") {
                    tabs.push(TabInfo {
                        id: t
                            .get("targetId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        url: t
                            .get("url")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        title: t
                            .get("title")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    });
                }
            }
        }
        Ok(tabs)
    }

    async fn switch_tab(&self, tab_id: &str) -> Result<(), CliError> {
        self.send_cdp("Target.activateTarget", json!({ "targetId": tab_id }))
            .await?;
        Ok(())
    }

    async fn close(&self) -> Result<(), CliError> {
        self.send_cdp("Browser.close", json!({})).await.ok();
        Ok(())
    }

    async fn intercept_requests(&self, url_pattern: &str) -> Result<(), CliError> {
        let js = dom_helpers::install_interceptor_js(url_pattern);
        self.evaluate_js(&js, false).await?;
        Ok(())
    }

    async fn get_intercepted_requests(&self) -> Result<Vec<InterceptedRequest>, CliError> {
        let js = dom_helpers::get_intercepted_requests_js();
        let val = self.evaluate_js(&js, false).await?;
        let reqs: Vec<InterceptedRequest> = serde_json::from_value(val).unwrap_or_default();
        Ok(reqs)
    }

    async fn get_network_requests(&self) -> Result<Vec<NetworkRequest>, CliError> {
        let js = dom_helpers::network_requests_js();
        let val = self.evaluate_js(&js, false).await?;
        let reqs: Vec<NetworkRequest> = serde_json::from_value(val).unwrap_or_default();
        Ok(reqs)
    }
}
