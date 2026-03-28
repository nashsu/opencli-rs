//! Auth strategy probing: automatically discover the minimum-privilege strategy.
//!
//! Strategy Cascade: automatic strategy downgrade chain.
//!
//! Probes an API endpoint starting from the simplest strategy (PUBLIC)
//! and automatically downgrades through the strategy tiers until one works:
//!
//!   PUBLIC -> COOKIE -> HEADER -> INTERCEPT -> UI
//!
//! This eliminates the need for manual strategy selection -- the system
//! automatically finds the minimum-privilege strategy that works.

use opencli_rs_core::{CliError, IPage, Strategy};
use tracing::debug;

use crate::types::StrategyTestResult;

/// Result of the cascade auth-strategy probe (with confidence score).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CascadeResult {
    pub url: String,
    pub strategy: Strategy,
    pub confidence: f64,
    pub tested: Vec<StrategyTestResult>,
}

/// Strategy cascade order (simplest to most complex).
const CASCADE_ORDER: &[Strategy] = &[
    Strategy::Public,
    Strategy::Cookie,
    Strategy::Header,
    Strategy::Intercept,
];

// ── Probe JS builders ───────────────────────────────────────────────────────

/// Build the JavaScript source for a fetch probe.
/// Shared logic parameterized by credentials and CSRF extraction.
fn build_fetch_probe_js(url: &str, credentials: bool, extract_csrf: bool) -> String {
    let url_json = serde_json::to_string(url).unwrap_or_else(|_| format!("\"{}\"", url));

    let credentials_line = if credentials {
        "credentials: 'include',"
    } else {
        ""
    };

    let header_setup = if extract_csrf {
        r#"
      const cookies = document.cookie.split(';').map(c => c.trim());
      const csrf = cookies.find(c => c.startsWith('ct0=') || c.startsWith('csrf_token=') || c.startsWith('_csrf='))?.split('=').slice(1).join('=');
      const headers = {};
      if (csrf) { headers['X-Csrf-Token'] = csrf; headers['X-XSRF-Token'] = csrf; }
    "#
    } else {
        "const headers = {};"
    };

    format!(
        r#"
    async () => {{
      try {{
        {header_setup}
        const resp = await fetch({url_json}, {{
          {credentials_line}
          headers
        }});
        const status = resp.status;
        if (!resp.ok) return {{ status, ok: false }};
        const text = await resp.text();
        let hasData = false;
        try {{
          const json = JSON.parse(text);
          hasData = !!json && (Array.isArray(json) ? json.length > 0 :
            typeof json === 'object' && Object.keys(json).length > 0);
          // Check for API-level error codes (common in Chinese sites)
          if (json.code !== undefined && json.code !== 0) hasData = false;
        }} catch {{}}
        return {{ status, ok: true, hasData, preview: text.slice(0, 200) }};
      }} catch (e) {{ return {{ ok: false, error: e.message }}; }}
    }}
  "#,
        header_setup = header_setup,
        url_json = url_json,
        credentials_line = credentials_line,
    )
}

/// Probe an endpoint with a specific strategy.
/// Returns whether the probe succeeded and basic response info.
pub async fn probe_endpoint(page: &dyn IPage, url: &str, strategy: Strategy) -> StrategyTestResult {
    let result = match strategy {
        Strategy::Public => {
            // PUBLIC: plain fetch, no credentials
            let js = build_fetch_probe_js(url, false, false);
            eval_probe(page, &js, strategy).await
        }
        Strategy::Cookie => {
            // COOKIE: fetch with credentials: 'include'
            let js = build_fetch_probe_js(url, true, false);
            eval_probe(page, &js, strategy).await
        }
        Strategy::Header => {
            // HEADER: fetch with credentials + auto-extract CSRF tokens
            let js = build_fetch_probe_js(url, true, true);
            eval_probe(page, &js, strategy).await
        }
        Strategy::Intercept | Strategy::Ui => {
            // These require site-specific implementation
            StrategyTestResult {
                strategy,
                success: false,
                status_code: None,
                has_data: false,
            }
        }
    };

    result
}

/// Evaluate a probe JS and parse the result.
async fn eval_probe(page: &dyn IPage, js: &str, strategy: Strategy) -> StrategyTestResult {
    match page.evaluate(js).await {
        Ok(value) => {
            let ok = value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            let has_data = value
                .get("hasData")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let status_code = value
                .get("status")
                .and_then(|v| v.as_u64())
                .map(|s| s as u16);
            StrategyTestResult {
                strategy,
                success: ok && has_data,
                status_code,
                has_data,
            }
        }
        Err(e) => {
            debug!("Strategy {:?} probe failed: {}", strategy, e);
            StrategyTestResult {
                strategy,
                success: false,
                status_code: None,
                has_data: false,
            }
        }
    }
}

/// Run the cascade: try each strategy in order until one works.
///
/// 1. Navigate to site root to establish cookies
/// 2. Try strategies in order: PUBLIC -> COOKIE -> HEADER
/// 3. Return first successful strategy with confidence score
///
/// Confidence: 1.0 for PUBLIC, 0.9 for COOKIE, 0.8 for HEADER (simpler = more confident).
/// If none works, defaults to COOKIE with 0.3 confidence.
pub async fn cascade(page: &dyn IPage, api_url: &str) -> Result<CascadeResult, CliError> {
    // Don't auto-try INTERCEPT/UI -- stop at HEADER
    let max_idx = CASCADE_ORDER
        .iter()
        .position(|&s| s == Strategy::Header)
        .unwrap_or(CASCADE_ORDER.len() - 1);

    let mut tested = Vec::new();

    for (i, &strategy) in CASCADE_ORDER.iter().enumerate() {
        if i > max_idx {
            break;
        }

        let result = probe_endpoint(page, api_url, strategy).await;
        let success = result.success;
        tested.push(result);

        if success {
            let confidence = 1.0 - (i as f64 * 0.1);
            debug!(
                "Cascade found working strategy: {:?} (confidence: {:.1})",
                strategy, confidence
            );
            return Ok(CascadeResult {
                url: api_url.to_string(),
                strategy,
                confidence,
                tested,
            });
        }
    }

    // None worked -- default to Cookie (most common for logged-in sites)
    debug!("Cascade: no strategy worked, defaulting to Cookie");
    Ok(CascadeResult {
        url: api_url.to_string(),
        strategy: Strategy::Cookie,
        confidence: 0.3,
        tested,
    })
}

/// Render cascade results for display.
pub fn render_cascade_result(result: &CascadeResult) -> String {
    let mut lines = vec![format!(
        "Strategy Cascade: {} ({:.0}% confidence)",
        result.strategy,
        result.confidence * 100.0,
    )];
    for probe in &result.tested {
        let icon = if probe.success { "pass" } else { "fail" };
        let status = probe
            .status_code
            .map(|s| format!(" [{}]", s))
            .unwrap_or_default();
        lines.push(format!("  {} {}{}", icon, probe.strategy, status));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cascade_order_starts_with_public() {
        assert_eq!(CASCADE_ORDER[0], Strategy::Public);
    }

    #[test]
    fn test_cascade_order_ends_with_intercept() {
        assert_eq!(CASCADE_ORDER[CASCADE_ORDER.len() - 1], Strategy::Intercept);
    }

    #[test]
    fn test_public_probe_js_has_no_credentials() {
        let js = build_fetch_probe_js("https://api.example.com/data", false, false);
        assert!(!js.contains("credentials: 'include'"));
    }

    #[test]
    fn test_cookie_probe_js_has_credentials() {
        let js = build_fetch_probe_js("https://api.example.com/data", true, false);
        assert!(js.contains("credentials: 'include'"));
    }

    #[test]
    fn test_header_probe_js_extracts_csrf() {
        let js = build_fetch_probe_js("https://api.example.com/data", true, true);
        assert!(js.contains("csrf"));
        assert!(js.contains("X-Csrf-Token"));
        assert!(js.contains("X-XSRF-Token"));
        assert!(js.contains("credentials: 'include'"));
        assert!(js.contains("ct0="));
        assert!(js.contains("csrf_token="));
        assert!(js.contains("_csrf="));
    }

    #[test]
    fn test_probe_js_url_escaping() {
        let js = build_fetch_probe_js(
            "https://api.example.com/data?q=hello&limit=10",
            false,
            false,
        );
        assert!(js.contains("api.example.com"));
    }

    #[test]
    fn test_probe_js_checks_chinese_api_code() {
        let js = build_fetch_probe_js("https://api.example.com/data", false, false);
        assert!(js.contains("json.code !== undefined && json.code !== 0"));
    }

    #[test]
    fn test_render_cascade_result() {
        let result = CascadeResult {
            url: "https://api.example.com/data".to_string(),
            strategy: Strategy::Public,
            confidence: 1.0,
            tested: vec![StrategyTestResult {
                strategy: Strategy::Public,
                success: true,
                status_code: Some(200),
                has_data: true,
            }],
        };
        let rendered = render_cascade_result(&result);
        assert!(rendered.contains("Strategy Cascade"));
        assert!(rendered.contains("100%"));
    }
}
