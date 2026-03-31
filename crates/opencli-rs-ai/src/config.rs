//! Configuration file handling for opencli-rs.
//! Reads ~/.opencli-rs/config.json for LLM settings and other configuration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_API_BASE: &str = "https://www.autocli.ai";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub llm: LlmConfig,
    /// AutoCLI token for authenticated API access
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "autocli-token")]
    pub autocli_token: Option<String>,
}

/// Get the AutoCLI server base URL from env var or default.
pub fn api_base() -> String {
    std::env::var("AUTOCLI_API_BASE")
        .unwrap_or_else(|_| DEFAULT_API_BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Get the search endpoint URL
pub fn search_url(pattern: &str) -> String {
    format!("{}/api/sites/cli/search?url={}", api_base(), urlencoding::encode(pattern))
}

/// Get the upload endpoint URL
pub fn upload_url() -> String {
    format!("{}/api/sites/upload", api_base())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmConfig {
    /// API endpoint URL (e.g., "https://api.anthropic.com/v1/messages", "https://api.openai.com/v1/chat/completions")
    pub endpoint: Option<String>,
    /// API key
    pub apikey: Option<String>,
    /// Model name (e.g., "claude-sonnet-4-20250514", "gpt-4o")
    pub modelname: Option<String>,
}

impl LlmConfig {
    pub fn is_configured(&self) -> bool {
        self.endpoint.is_some() && self.apikey.is_some() && self.modelname.is_some()
    }
}

/// Get the config file path: ~/.opencli-rs/config.json
pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".opencli-rs").join("config.json")
}

/// Load config from ~/.opencli-rs/config.json
/// Returns default config if file doesn't exist or can't be parsed.
pub fn load_config() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Save config to ~/.opencli-rs/config.json
pub fn save_config(config: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, content).map_err(|e| format!("Failed to write config: {}", e))?;
    Ok(())
}
