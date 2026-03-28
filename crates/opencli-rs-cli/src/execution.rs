use opencli_rs_browser::BrowserBridge;
use opencli_rs_core::{CliCommand, CliError, IPage};
use opencli_rs_pipeline::{execute_pipeline, steps::register_all_steps, StepRegistry};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Get daemon port from env or default
fn daemon_port() -> u16 {
    std::env::var("OPENCLI_DAEMON_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(19825)
}

/// Get command timeout from env or command config or default (60s)
fn command_timeout(cmd: &CliCommand) -> u64 {
    std::env::var("OPENCLI_BROWSER_COMMAND_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .or(cmd.timeout_seconds)
        .unwrap_or(60)
}

pub async fn execute_command(
    cmd: &CliCommand,
    kwargs: HashMap<String, Value>,
) -> Result<Value, CliError> {
    tracing::info!(site = %cmd.site, name = %cmd.name, "Executing command");

    let timeout_secs = command_timeout(cmd);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        execute_command_inner(cmd, kwargs),
    )
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(CliError::timeout(format!(
            "Command '{}' timed out after {}s",
            cmd.full_name(),
            timeout_secs
        ))),
    }
}

async fn execute_command_inner(
    cmd: &CliCommand,
    kwargs: HashMap<String, Value>,
) -> Result<Value, CliError> {
    // Build step registry
    let mut registry = StepRegistry::new();
    register_all_steps(&mut registry);

    if cmd.needs_browser() {
        // Browser session
        let mut bridge = BrowserBridge::new(daemon_port());
        let page = bridge.connect().await?;

        // Pre-navigate to domain if set, but ONLY if the pipeline doesn't
        // start with its own navigate step (to avoid double navigation).
        let pipeline_starts_with_navigate = cmd
            .pipeline
            .as_ref()
            .and_then(|steps| steps.first())
            .and_then(|step| step.as_object())
            .is_some_and(|obj| obj.contains_key("navigate"));

        if !pipeline_starts_with_navigate {
            if let Some(domain) = &cmd.domain {
                let url = format!("https://{}", domain);
                tracing::debug!(url = %url, "Pre-navigating to domain");
                page.goto(&url, None).await?;
            }
        }

        // Execute
        let result = if let Some(ref steps) = cmd.pipeline {
            execute_pipeline(Some(page.clone()), steps, &kwargs, &registry).await
        } else if cmd.func.is_some() {
            run_command(cmd, Some(page.clone()), &kwargs, &registry).await
        } else {
            Err(CliError::command_execution(format!(
                "Command '{}' has no pipeline or func",
                cmd.full_name()
            )))
        };

        // Close the automation tab/window after command completes
        let _ = page.close().await;

        result
    } else {
        run_command(cmd, None, &kwargs, &registry).await
    }
}

async fn run_command(
    cmd: &CliCommand,
    page: Option<Arc<dyn IPage>>,
    kwargs: &HashMap<String, Value>,
    registry: &StepRegistry,
) -> Result<Value, CliError> {
    if let Some(pipeline) = &cmd.pipeline {
        execute_pipeline(page, pipeline, kwargs, registry).await
    } else if let Some(func) = &cmd.func {
        func(page, kwargs.clone()).await
    } else {
        Err(CliError::command_execution(format!(
            "Command '{}' has no pipeline or func",
            cmd.full_name()
        )))
    }
}
