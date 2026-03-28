use opencli_rs_core::{CliError, IPage};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

use crate::step_registry::StepRegistry;

const MAX_BROWSER_ATTEMPTS: usize = 3;

/// Execute a pipeline — a sequence of steps.
///
/// Each step is a YAML object like `{ "fetch": "https://..." }` or
/// `{ "map": { "title": "${{ item.title }}" } }`.
///
/// Steps are executed sequentially. Each step receives the current `data` and
/// returns new `data`. Browser steps get up to 2 retries on transient errors.
pub async fn execute_pipeline(
    page: Option<Arc<dyn IPage>>,
    pipeline: &[Value],
    args: &HashMap<String, Value>,
    registry: &StepRegistry,
) -> Result<Value, CliError> {
    let mut data = Value::Null;

    for (i, step) in pipeline.iter().enumerate() {
        let obj = step
            .as_object()
            .ok_or_else(|| CliError::pipeline(format!("Step {i} is not an object: {step}")))?;

        if obj.len() != 1 {
            return Err(CliError::pipeline(format!(
                "Step {i} must have exactly one key, found {}",
                obj.len()
            )));
        }

        let (step_name, params) = obj.iter().next().unwrap();

        let handler = registry.get(step_name).ok_or_else(|| {
            CliError::pipeline(format!("Unknown step '{step_name}' at index {i}"))
        })?;

        let is_browser = handler.is_browser_step();
        let mut last_error: Option<CliError> = None;

        for attempt in 0..if is_browser { MAX_BROWSER_ATTEMPTS } else { 1 } {
            match handler.execute(page.clone(), params, &data, args).await {
                Ok(result) => {
                    data = result;
                    last_error = None;
                    break;
                }
                Err(e) => {
                    if is_browser && attempt + 1 < MAX_BROWSER_ATTEMPTS {
                        warn!(
                            step = step_name,
                            attempt = attempt + 1,
                            "Browser step failed, retrying: {e}"
                        );
                        last_error = Some(e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        if let Some(e) = last_error {
            return Err(e);
        }
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::step_registry::StepHandler;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct EchoStep;

    #[async_trait]
    impl StepHandler for EchoStep {
        fn name(&self) -> &'static str {
            "echo"
        }

        async fn execute(
            &self,
            _page: Option<Arc<dyn IPage>>,
            params: &Value,
            _data: &Value,
            _args: &HashMap<String, Value>,
        ) -> Result<Value, CliError> {
            Ok(params.clone())
        }
    }

    /// A step that appends its params to the current data array, or wraps data
    /// in an array if it is not already one.
    struct AppendStep;

    #[async_trait]
    impl StepHandler for AppendStep {
        fn name(&self) -> &'static str {
            "append"
        }

        async fn execute(
            &self,
            _page: Option<Arc<dyn IPage>>,
            params: &Value,
            data: &Value,
            _args: &HashMap<String, Value>,
        ) -> Result<Value, CliError> {
            let mut arr = match data {
                Value::Array(a) => a.clone(),
                Value::Null => vec![],
                other => vec![other.clone()],
            };
            arr.push(params.clone());
            Ok(Value::Array(arr))
        }
    }

    /// A browser step that fails the first N times, then succeeds.
    struct FlakyBrowserStep {
        fail_count: AtomicUsize,
        fail_times: usize,
    }

    impl FlakyBrowserStep {
        fn new(fail_times: usize) -> Self {
            Self {
                fail_count: AtomicUsize::new(0),
                fail_times,
            }
        }
    }

    #[async_trait]
    impl StepHandler for FlakyBrowserStep {
        fn name(&self) -> &'static str {
            "flaky_browser"
        }

        fn is_browser_step(&self) -> bool {
            true
        }

        async fn execute(
            &self,
            _page: Option<Arc<dyn IPage>>,
            params: &Value,
            _data: &Value,
            _args: &HashMap<String, Value>,
        ) -> Result<Value, CliError> {
            let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
            if count < self.fail_times {
                Err(CliError::pipeline("transient browser error"))
            } else {
                Ok(params.clone())
            }
        }
    }

    fn empty_args() -> HashMap<String, Value> {
        HashMap::new()
    }

    #[tokio::test]
    async fn empty_pipeline_returns_null() {
        let registry = StepRegistry::new();
        let result = execute_pipeline(None, &[], &empty_args(), &registry)
            .await
            .unwrap();
        assert_eq!(result, Value::Null);
    }

    #[tokio::test]
    async fn single_step_returns_step_output() {
        let mut registry = StepRegistry::new();
        registry.register(Arc::new(EchoStep));

        let pipeline = vec![json!({"echo": "hello"})];
        let result = execute_pipeline(None, &pipeline, &empty_args(), &registry)
            .await
            .unwrap();
        assert_eq!(result, json!("hello"));
    }

    #[tokio::test]
    async fn multi_step_pipeline_chains_data() {
        let mut registry = StepRegistry::new();
        registry.register(Arc::new(AppendStep));

        let pipeline = vec![
            json!({"append": "first"}),
            json!({"append": "second"}),
            json!({"append": "third"}),
        ];
        let result = execute_pipeline(None, &pipeline, &empty_args(), &registry)
            .await
            .unwrap();
        assert_eq!(result, json!(["first", "second", "third"]));
    }

    #[tokio::test]
    async fn unknown_step_returns_error() {
        let registry = StepRegistry::new();
        let pipeline = vec![json!({"nonexistent": null})];
        let err = execute_pipeline(None, &pipeline, &empty_args(), &registry)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent"),
            "Error should mention the step name: {msg}"
        );
    }

    #[tokio::test]
    async fn browser_step_retries_on_transient_error() {
        let mut registry = StepRegistry::new();
        // Fails twice then succeeds — should succeed on 3rd attempt
        registry.register(Arc::new(FlakyBrowserStep::new(2)));

        let pipeline = vec![json!({"flaky_browser": "ok"})];
        let result = execute_pipeline(None, &pipeline, &empty_args(), &registry)
            .await
            .unwrap();
        assert_eq!(result, json!("ok"));
    }

    #[tokio::test]
    async fn browser_step_fails_after_max_retries() {
        let mut registry = StepRegistry::new();
        // Fails 3 times — all 3 attempts exhausted
        registry.register(Arc::new(FlakyBrowserStep::new(3)));

        let pipeline = vec![json!({"flaky_browser": "ok"})];
        let err = execute_pipeline(None, &pipeline, &empty_args(), &registry)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("transient browser error"));
    }
}
