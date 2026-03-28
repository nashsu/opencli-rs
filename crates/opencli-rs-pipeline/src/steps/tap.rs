use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use opencli_rs_core::{CliError, IPage};
use serde_json::Value;

use crate::step_registry::{StepHandler, StepRegistry};
use crate::template::{render_template_str, TemplateContext};

// ---------------------------------------------------------------------------
// TapStep — Store Action Bridge (Pinia/Vuex)
// ---------------------------------------------------------------------------

/// TapStep bridges store actions (Pinia/Vuex) with network interception.
///
/// Generates a self-contained IIFE that:
/// 1. Installs fetch + XHR dual interception proxy
/// 2. Finds the Pinia/Vuex store and calls the action
/// 3. Captures the response matching the URL pattern
/// 4. Auto-cleans up interception in finally block
/// 5. Returns the captured data (optionally sub-selected)
pub struct TapStep;

#[async_trait]
impl StepHandler for TapStep {
    fn name(&self) -> &'static str {
        "tap"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = page
            .clone()
            .ok_or_else(|| CliError::pipeline("tap: requires an active page"))?;

        let obj = params
            .as_object()
            .ok_or_else(|| CliError::pipeline("tap: params must be an object"))?;

        let ctx = TemplateContext {
            args: args.clone(),
            data: data.clone(),
            item: Value::Null,
            index: 0,
        };

        // Extract store name (required)
        let store_name = obj
            .get("store")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CliError::pipeline("tap: missing 'store' field"))?;
        let store_name = render_template_str(store_name, &ctx)?
            .as_str()
            .unwrap_or("")
            .to_string();

        // Extract action name (required)
        let action_name = obj
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CliError::pipeline("tap: missing 'action' field"))?;
        let action_name = render_template_str(action_name, &ctx)?
            .as_str()
            .unwrap_or("")
            .to_string();

        // Extract capture URL pattern (supports "capture" and "url" field names)
        let capture_pattern = obj
            .get("capture")
            .or_else(|| obj.get("url"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let capture_pattern = render_template_str(capture_pattern, &ctx)?
            .as_str()
            .unwrap_or("")
            .to_string();

        // Extract timeout in seconds
        let timeout_secs = obj
            .get("timeout")
            .or_else(|| obj.get("wait"))
            .and_then(|v| v.as_f64())
            .unwrap_or(5.0);

        // Extract select path (optional)
        let select_path = obj.get("select").and_then(|v| v.as_str());
        let select_chain = match select_path {
            Some(path) => path
                .split('.')
                .map(|p| format!("?.[{}]", serde_json::to_string(p).unwrap_or_default()))
                .collect::<String>(),
            None => String::new(),
        };

        // Extract framework hint (optional)
        let framework = obj
            .get("framework")
            .and_then(|v| v.as_str())
            .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "null".to_string()))
            .unwrap_or_else(|| "null".to_string());

        // Extract action args (optional)
        let action_args = obj.get("args").cloned().unwrap_or(Value::Array(vec![]));

        let store_name_json = serde_json::to_string(&store_name).unwrap_or("\"\"".to_string());
        let action_name_json = serde_json::to_string(&action_name).unwrap_or("\"\"".to_string());
        let capture_json = serde_json::to_string(&capture_pattern).unwrap_or("\"\"".to_string());

        // Build the action call
        let action_call = if action_args == Value::Array(vec![]) {
            format!("store[{action_name_json}]()")
        } else {
            let rendered_args: Vec<String> = action_args
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|a| serde_json::to_string(a).unwrap_or("null".to_string()))
                .collect();
            format!("store[{action_name_json}]({})", rendered_args.join(", "))
        };

        // Generate self-contained JS that does everything in the browser
        let js = format!(
            r#"(async () => {{
  // ── 1. Setup capture proxy (fetch + XHR dual interception) ──
  let captured = null;
  let captureResolve;
  const capturePromise = new Promise(r => {{ captureResolve = r; }});
  const capturePattern = {capture_json};

  const origFetch = window.fetch;
  window.fetch = async function(...fetchArgs) {{
    const resp = await origFetch.apply(this, fetchArgs);
    try {{
      const url = typeof fetchArgs[0] === 'string' ? fetchArgs[0]
        : fetchArgs[0] instanceof Request ? fetchArgs[0].url : String(fetchArgs[0]);
      if (capturePattern && url.includes(capturePattern) && !captured) {{
        try {{ captured = await resp.clone().json(); captureResolve(); }} catch {{}}
      }}
    }} catch {{}}
    return resp;
  }};

  const origXhrOpen = XMLHttpRequest.prototype.open;
  const origXhrSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function(method, url) {{
    this.__tapUrl = String(url);
    return origXhrOpen.apply(this, arguments);
  }};
  XMLHttpRequest.prototype.send = function(body) {{
    if (capturePattern && this.__tapUrl?.includes(capturePattern)) {{
      this.addEventListener('load', function() {{
        if (!captured) {{
          try {{ captured = JSON.parse(this.responseText); captureResolve(); }} catch {{}}
        }}
      }});
    }}
    return origXhrSend.apply(this, arguments);
  }};

  try {{
    // ── 2. Find store ──
    let store = null;
    const storeName = {store_name_json};
    const fw = {framework};

    const app = document.querySelector('#app');
    if (!fw || fw === 'pinia') {{
      try {{
        const pinia = app?.__vue_app__?.config?.globalProperties?.$pinia;
        if (pinia?._s) store = pinia._s.get(storeName);
      }} catch {{}}
    }}
    if (!store && (!fw || fw === 'vuex')) {{
      try {{
        const vuexStore = app?.__vue_app__?.config?.globalProperties?.$store
          ?? app?.__vue__?.$store;
        if (vuexStore) {{
          store = {{ [{action_name_json}]: (...a) => vuexStore.dispatch(storeName + '/' + {action_name_json}, ...a) }};
        }}
      }} catch {{}}
    }}

    if (!store) return {{ error: 'Store not found: ' + storeName, hint: 'Page may not be fully loaded or store name may be incorrect' }};
    if (typeof store[{action_name_json}] !== 'function') {{
      return {{ error: 'Action not found: ' + {action_name_json} + ' on store ' + storeName,
        hint: 'Available: ' + Object.keys(store).filter(k => typeof store[k] === 'function' && !k.startsWith('$') && !k.startsWith('_')).join(', ') }};
    }}

    // ── 3. Call store action ──
    await {action_call};

    // ── 4. Wait for network response ──
    if (!captured) {{
      const timeoutPromise = new Promise(r => setTimeout(r, {timeout_ms}));
      await Promise.race([capturePromise, timeoutPromise]);
    }}
  }} finally {{
    // ── 5. Always restore originals ──
    window.fetch = origFetch;
    XMLHttpRequest.prototype.open = origXhrOpen;
    XMLHttpRequest.prototype.send = origXhrSend;
  }}

  if (!captured) return {{ error: 'No matching response captured for pattern: ' + capturePattern }};
  return captured{select_chain} ?? captured;
}})()"#,
            timeout_ms = (timeout_secs * 1000.0) as u64,
        );

        let result = pg.evaluate(&js).await?;

        // Check if the result is an error object from the JS
        if let Some(error) = result.get("error").and_then(|v| v.as_str()) {
            let hint = result.get("hint").and_then(|v| v.as_str()).unwrap_or("");
            return Err(CliError::command_execution(format!(
                "tap: {} {}",
                error,
                if hint.is_empty() {
                    String::new()
                } else {
                    format!("({})", hint)
                }
            )));
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register_tap_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(TapStep));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_tap_step_registers() {
        let mut registry = StepRegistry::new();
        register_tap_steps(&mut registry);
        assert!(registry.get("tap").is_some());
    }

    #[test]
    fn test_tap_is_browser_step() {
        assert!(TapStep.is_browser_step());
    }

    #[tokio::test]
    async fn test_tap_requires_page() {
        let step = TapStep;
        let params = json!({"store": "feed", "action": "fetchData"});
        let result = step
            .execute(None, &params, &json!(null), &HashMap::new())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tap_requires_object_params() {
        let step = TapStep;
        let result = step
            .execute(None, &json!("invalid"), &json!(null), &HashMap::new())
            .await;
        assert!(result.is_err());
    }
}
