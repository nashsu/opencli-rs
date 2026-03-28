use crate::{ArgDef, CliError, IPage, Strategy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type CommandArgs = HashMap<String, Value>;

pub type AdapterFunc = Arc<
    dyn Fn(
            Option<Arc<dyn IPage>>,
            CommandArgs,
        ) -> Pin<Box<dyn Future<Output = Result<Value, CliError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NavigateBefore {
    Bool(bool),
    Url(String),
}

impl Default for NavigateBefore {
    fn default() -> Self {
        Self::Bool(true)
    }
}

#[derive(Clone)]
pub struct CliCommand {
    pub site: String,
    pub name: String,
    pub description: String,
    pub domain: Option<String>,
    pub strategy: Strategy,
    pub browser: bool,
    pub args: Vec<ArgDef>,
    pub columns: Vec<String>,
    pub pipeline: Option<Vec<Value>>,
    pub func: Option<AdapterFunc>,
    pub timeout_seconds: Option<u64>,
    pub navigate_before: NavigateBefore,
}

impl CliCommand {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.site, self.name)
    }

    pub fn needs_browser(&self) -> bool {
        if self.browser || self.strategy.requires_browser() {
            return true;
        }
        // Check if pipeline contains browser steps
        if let Some(ref pipeline) = self.pipeline {
            const BROWSER_STEPS: &[&str] = &[
                "navigate",
                "click",
                "type",
                "wait",
                "press",
                "evaluate",
                "snapshot",
                "screenshot",
                "intercept",
                "tap",
            ];
            for step in pipeline {
                if let Some(obj) = step.as_object() {
                    for key in obj.keys() {
                        if BROWSER_STEPS.contains(&key.as_str()) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

impl std::fmt::Debug for CliCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliCommand")
            .field("site", &self.site)
            .field("name", &self.name)
            .field("strategy", &self.strategy)
            .field("browser", &self.browser)
            .field("has_func", &self.func.is_some())
            .field("has_pipeline", &self.pipeline.is_some())
            .finish()
    }
}
