use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use opencli_rs_core::{CliError, IPage};
use serde_json::Value;

use crate::step_registry::{StepHandler, StepRegistry};
use crate::template::{render_template_str, TemplateContext};

// ---------------------------------------------------------------------------
// InterceptStep
// ---------------------------------------------------------------------------

pub struct InterceptStep;

#[async_trait]
impl StepHandler for InterceptStep {
    fn name(&self) -> &'static str {
        "intercept"
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
            .ok_or_else(|| CliError::pipeline("intercept: requires an active page"))?;

        let ctx = TemplateContext {
            args: args.clone(),
            data: data.clone(),
            item: Value::Null,
            index: 0,
        };

        let (pattern, wait_ms, install_only) = match params {
            Value::String(s) => {
                let rendered = render_template_str(s, &ctx)?;
                let pat = rendered
                    .as_str()
                    .ok_or_else(|| {
                        CliError::pipeline("intercept: pattern must resolve to a string")
                    })?
                    .to_string();
                (pat, 5000u64, false)
            }
            Value::Object(obj) => {
                let pat_raw = obj
                    .get("pattern")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CliError::pipeline("intercept: missing 'pattern' field"))?;
                let rendered = render_template_str(pat_raw, &ctx)?;
                let pat = rendered
                    .as_str()
                    .ok_or_else(|| {
                        CliError::pipeline("intercept: pattern must resolve to a string")
                    })?
                    .to_string();
                let wait = obj
                    .get("wait")
                    .and_then(|v| v.as_f64())
                    .map(|s| (s * 1000.0) as u64)
                    .unwrap_or(5000);
                // If collect: false, only install the interceptor without waiting/collecting
                let install_only = obj
                    .get("collect")
                    .and_then(|v| v.as_bool())
                    .map(|b| !b)
                    .unwrap_or(false);
                (pat, wait, install_only)
            }
            _ => {
                return Err(CliError::pipeline(
                    "intercept: params must be a string pattern or object with 'pattern'",
                ))
            }
        };

        // Install the interceptor
        pg.intercept_requests(&pattern).await?;

        // If install-only mode, return data unchanged (collect step will handle later)
        if install_only {
            return Ok(data.clone());
        }

        // Wait for the specified duration to capture requests
        pg.wait_for_timeout(wait_ms).await?;

        // Collect intercepted requests
        let requests = pg.get_intercepted_requests().await?;
        let result: Vec<Value> = requests
            .into_iter()
            .map(|r| serde_json::to_value(&r).unwrap_or(Value::Null))
            .collect();

        Ok(Value::Array(result))
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register_intercept_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(InterceptStep));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_intercept_step_registers() {
        let mut registry = StepRegistry::new();
        register_intercept_steps(&mut registry);
        assert!(registry.get("intercept").is_some());
    }

    #[test]
    fn test_intercept_is_browser_step() {
        assert!(InterceptStep.is_browser_step());
    }

    #[tokio::test]
    async fn test_intercept_requires_page() {
        let step = InterceptStep;
        let result = step
            .execute(None, &json!("*/api/*"), &json!(null), &HashMap::new())
            .await;
        assert!(result.is_err());
    }
}
