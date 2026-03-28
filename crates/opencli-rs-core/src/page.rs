use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::CliError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GotoOptions {
    #[serde(default)]
    pub wait_until: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CookieOptions {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub expires: Option<f64>,
    #[serde(default)]
    pub http_only: Option<bool>,
    #[serde(default)]
    pub secure: Option<bool>,
    #[serde(default)]
    pub same_site: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotOptions {
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub include_hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollDirection {
    #[default]
    Down,
    Up,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoScrollOptions {
    #[serde(default)]
    pub direction: ScrollDirection,
    #[serde(default)]
    pub max_scrolls: Option<u32>,
    #[serde(default)]
    pub delay_ms: Option<u64>,
    #[serde(default)]
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WaitOptions {
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkRequest {
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub response_body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptedRequest {
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScreenshotOptions {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub full_page: bool,
    #[serde(default)]
    pub selector: Option<String>,
}

#[async_trait]
pub trait IPage: Send + Sync {
    /// Navigate to a URL
    async fn goto(&self, url: &str, options: Option<GotoOptions>) -> Result<(), CliError>;

    /// Get the current URL
    async fn url(&self) -> Result<String, CliError>;

    /// Get the page title
    async fn title(&self) -> Result<String, CliError>;

    /// Get the full HTML content
    async fn content(&self) -> Result<String, CliError>;

    /// Evaluate a JavaScript expression and return the result
    async fn evaluate(&self, expression: &str) -> Result<Value, CliError>;

    /// Wait for a selector to appear
    async fn wait_for_selector(
        &self,
        selector: &str,
        options: Option<WaitOptions>,
    ) -> Result<(), CliError>;

    /// Wait for navigation to complete
    async fn wait_for_navigation(&self, options: Option<WaitOptions>) -> Result<(), CliError>;

    /// Wait for a fixed delay (milliseconds)
    async fn wait_for_timeout(&self, ms: u64) -> Result<(), CliError>;

    /// Click on an element
    async fn click(&self, selector: &str) -> Result<(), CliError>;

    /// Type text into an element
    async fn type_text(&self, selector: &str, text: &str) -> Result<(), CliError>;

    /// Get cookies
    async fn cookies(&self, options: Option<CookieOptions>) -> Result<Vec<Cookie>, CliError>;

    /// Set cookies
    async fn set_cookies(&self, cookies: Vec<Cookie>) -> Result<(), CliError>;

    /// Take a screenshot
    async fn screenshot(&self, options: Option<ScreenshotOptions>) -> Result<Vec<u8>, CliError>;

    /// Get an accessibility snapshot of the page
    async fn snapshot(&self, options: Option<SnapshotOptions>) -> Result<Value, CliError>;

    /// Auto-scroll the page
    async fn auto_scroll(&self, options: Option<AutoScrollOptions>) -> Result<(), CliError>;

    /// List open tabs
    async fn tabs(&self) -> Result<Vec<TabInfo>, CliError>;

    /// Switch to a tab by ID
    async fn switch_tab(&self, tab_id: &str) -> Result<(), CliError>;

    /// Close the page
    async fn close(&self) -> Result<(), CliError>;

    /// Start intercepting network requests matching a URL pattern
    async fn intercept_requests(&self, url_pattern: &str) -> Result<(), CliError>;

    /// Get intercepted requests
    async fn get_intercepted_requests(&self) -> Result<Vec<InterceptedRequest>, CliError>;

    /// Get network requests (captured)
    async fn get_network_requests(&self) -> Result<Vec<NetworkRequest>, CliError>;
}
