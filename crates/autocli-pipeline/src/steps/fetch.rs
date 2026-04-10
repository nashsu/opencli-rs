use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, StreamExt};
use autocli_core::{CliError, IPage};
use serde_json::Value;

use crate::step_registry::{StepHandler, StepRegistry};
use crate::template::{render_template, render_template_str, TemplateContext};

/// Helper to create an HTTP CliError.
fn http_error(msg: impl Into<String>) -> CliError {
    CliError::Http {
        message: msg.into(),
        suggestions: vec![],
        source: None,
    }
}

/// Render query params from a template object and append them to the URL.
fn append_query_params(
    url: &str,
    query_params: &Value,
    ctx: &TemplateContext,
) -> Result<String, CliError> {
    let obj = match query_params.as_object() {
        Some(o) => o,
        None => return Ok(url.to_string()),
    };
    if obj.is_empty() {
        return Ok(url.to_string());
    }

    let mut pairs = Vec::new();
    for (key, val_template) in obj {
        let rendered = render_template(val_template, ctx)?;
        let val_str = match &rendered {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => continue,
            other => other.to_string(),
        };
        pairs.push(format!(
            "{}={}",
            urlencoding::encode(key),
            urlencoding::encode(&val_str)
        ));
    }

    let separator = if url.contains('?') { "&" } else { "?" };
    Ok(format!("{url}{separator}{}", pairs.join("&")))
}

/// Check if a URL template references `item` (indicating per-item mode).
fn is_per_item_url(url: &str) -> bool {
    // Look for ${{ ... item ... }} patterns
    let mut rest = url;
    while let Some(pos) = rest.find("${{") {
        let after_marker = &rest[pos + 3..];
        if let Some(end) = after_marker.find("}}") {
            let expr = &after_marker[..end];
            // Check if the expression references `item` as a word
            if contains_item_ref(expr) {
                return true;
            }
            rest = &after_marker[end + 2..];
        } else {
            break;
        }
    }
    false
}

/// Check if an expression string contains an `item` reference (not `items` etc).
fn contains_item_ref(expr: &str) -> bool {
    let expr = expr.trim();
    for (i, _) in expr.match_indices("item") {
        // Check character before
        if i > 0 {
            let prev = expr.as_bytes()[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        // Check character after "item" (4 chars)
        let after_pos = i + 4;
        if after_pos < expr.len() {
            let next = expr.as_bytes()[after_pos];
            // Allow `.` (item.field), `[` (item[0]), `}` (end), whitespace, `|`
            if next.is_ascii_alphanumeric() || next == b'_' {
                continue;
            }
        }
        return true;
    }
    false
}

/// Execute a single HTTP request with a given client and return the JSON response.
async fn do_request_with_client(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    headers: Option<&Value>,
    body: Option<&Value>,
) -> Result<Value, CliError> {
    let method_upper = method.to_uppercase();
    let reqwest_method = match method_upper.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "PATCH" => reqwest::Method::PATCH,
        "DELETE" => reqwest::Method::DELETE,
        "HEAD" => reqwest::Method::HEAD,
        "OPTIONS" => reqwest::Method::OPTIONS,
        other => return Err(http_error(format!("unsupported HTTP method: {other}"))),
    };

    let mut request = client.request(reqwest_method, url);

    // Apply headers
    if let Some(Value::Object(hdrs)) = headers {
        for (key, val) in hdrs {
            if let Some(v) = val.as_str() {
                request = request.header(key.as_str(), v);
            }
        }
    }

    // Apply body
    if let Some(body_val) = body {
        match body_val {
            Value::String(s) => {
                request = request.body(s.clone());
            }
            other => {
                request = request.json(other);
            }
        }
    }

    let resp = request
        .send()
        .await
        .map_err(|e| http_error(format!("request failed: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(http_error(format!("HTTP {status}: {body_text}")));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| http_error(format!("failed to parse response as JSON: {e}")))?;

    Ok(json)
}

pub struct FetchStep {
    client: reqwest::Client,
}

impl FetchStep {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("autocli/0.1")
                .build()
                .unwrap_or_default(),
        }
    }

}

impl Default for FetchStep {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StepHandler for FetchStep {
    fn name(&self) -> &'static str {
        "fetch"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        // Extract URL, method, headers, body from params
        let (url_template, method, headers_template, body_template, query_params_template) = match params {
            // Mode 1: simple URL string
            Value::String(url) => (url.clone(), "GET".to_string(), None, None, None),
            // Mode 2/3: object params
            Value::Object(obj) => {
                let url = obj
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CliError::pipeline("fetch: object params must have 'url' field"))?
                    .to_string();
                let method = obj
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("GET")
                    .to_string();
                let headers = obj.get("headers").cloned();
                let body = obj.get("body").cloned();
                let query_params = obj.get("params").cloned();
                (url, method, headers, body, query_params)
            }
            _ => return Err(CliError::pipeline("fetch: params must be a string URL or an object")),
        };

        // Check for per-item mode: data is array AND url references item
        let is_array = data.as_array().is_some();
        let has_item_ref = is_per_item_url(&url_template);

        if is_array && has_item_ref {
            // Mode 3: per-item concurrent fetch
            let items = data.as_array().unwrap();
            let client = self.client.clone();
            let results: Vec<Result<Value, CliError>> = stream::iter(items.iter().cloned().enumerate())
                .map(|(index, item)| {
                    let url_tmpl = url_template.clone();
                    let method = method.clone();
                    let headers_tmpl = headers_template.clone();
                    let body_tmpl = body_template.clone();
                    let qp_tmpl = query_params_template.clone();
                    let args = args.clone();
                    let data = data.clone();
                    let client = client.clone();
                    async move {
                        let ctx = TemplateContext {
                            args,
                            data,
                            item,
                            index,
                        };
                        let rendered_url = render_template_str(&url_tmpl, &ctx)?;
                        let mut url_str = rendered_url
                            .as_str()
                            .ok_or_else(|| CliError::pipeline("fetch: rendered URL is not a string"))?
                            .to_string();

                        // Append query params if present
                        if let Some(qp) = &qp_tmpl {
                            url_str = append_query_params(&url_str, qp, &ctx)?;
                        }

                        // Render headers if present
                        let rendered_headers = match &headers_tmpl {
                            Some(h) => Some(render_template(h, &ctx)?),
                            None => None,
                        };

                        // Render body if present
                        let rendered_body = match &body_tmpl {
                            Some(b) => Some(render_template(b, &ctx)?),
                            None => None,
                        };

                        do_request_with_client(
                            &client,
                            &url_str,
                            &method,
                            rendered_headers.as_ref(),
                            rendered_body.as_ref(),
                        )
                        .await
                    }
                })
                .buffer_unordered(10)
                .collect()
                .await;

            // Collect results, propagating errors
            let mut output = Vec::with_capacity(results.len());
            for r in results {
                output.push(r?);
            }
            Ok(Value::Array(output))
        } else {
            // Mode 1 or 2: single request
            let ctx = TemplateContext {
                args: args.clone(),
                data: data.clone(),
                item: Value::Null,
                index: 0,
            };

            let rendered_url = render_template_str(&url_template, &ctx)?;
            let mut url_str = rendered_url
                .as_str()
                .ok_or_else(|| CliError::pipeline("fetch: rendered URL is not a string"))?
                .to_string();

            // Append query params if present
            if let Some(qp) = &query_params_template {
                url_str = append_query_params(&url_str, qp, &ctx)?;
            }

            // Render headers if present
            let rendered_headers = match &headers_template {
                Some(h) => Some(render_template(h, &ctx)?),
                None => None,
            };

            // Render body if present
            let rendered_body = match &body_template {
                Some(b) => Some(render_template(b, &ctx)?),
                None => None,
            };

            do_request_with_client(
                &self.client,
                &url_str,
                &method,
                rendered_headers.as_ref(),
                rendered_body.as_ref(),
            )
            .await
        }
    }
}

pub fn register_fetch_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(FetchStep::new()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_fetch_step_registers() {
        let mut registry = StepRegistry::new();
        register_fetch_steps(&mut registry);
        assert!(registry.get("fetch").is_some());
    }

    #[test]
    fn test_is_per_item_url() {
        assert!(is_per_item_url("https://api.com/${{ item.id }}"));
        assert!(is_per_item_url("https://api.com/${{item.id}}"));
        assert!(is_per_item_url("https://api.com/${{ item }}"));
        assert!(!is_per_item_url("https://api.com/${{ args.query }}"));
        assert!(!is_per_item_url("https://api.com/${{ items.length }}"));
        assert!(!is_per_item_url("https://api.com/plain-url"));
    }

    #[test]
    fn test_contains_item_ref() {
        assert!(contains_item_ref("item.id"));
        assert!(contains_item_ref(" item.id "));
        assert!(contains_item_ref("item"));
        assert!(!contains_item_ref("items"));
        assert!(!contains_item_ref("my_item"));
    }

    #[test]
    fn test_fetch_step_name() {
        let step = FetchStep::new();
        assert_eq!(step.name(), "fetch");
    }

    #[tokio::test]
    async fn test_fetch_rejects_invalid_params() {
        let step = FetchStep::new();
        let result = step
            .execute(None, &json!(42), &json!(null), &HashMap::new())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fetch_object_requires_url() {
        let step = FetchStep::new();
        let params = json!({"method": "POST"});
        let result = step
            .execute(None, &params, &json!(null), &HashMap::new())
            .await;
        assert!(result.is_err());
    }
}
