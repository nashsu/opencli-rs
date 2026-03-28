use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use opencli_rs_core::{CliError, IPage};
use serde_json::Value;

use crate::step_registry::{StepHandler, StepRegistry};
use crate::template::{render_template, render_template_str, TemplateContext};

// ---------------------------------------------------------------------------
// SelectStep
// ---------------------------------------------------------------------------

pub struct SelectStep;

#[async_trait]
impl StepHandler for SelectStep {
    fn name(&self) -> &'static str {
        "select"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        _args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let path = params
            .as_str()
            .ok_or_else(|| CliError::pipeline("select: params must be a string path"))?;

        let mut current = data.clone();
        for segment in parse_path_segments(path) {
            current = match segment {
                PathSegment::Key(key) => current.get(&key).cloned().unwrap_or(Value::Null),
                PathSegment::Index(idx) => current.get(idx).cloned().unwrap_or(Value::Null),
            };
        }

        Ok(current)
    }
}

enum PathSegment {
    Key(String),
    Index(usize),
}

/// Parse a dotted path like `"data.results[0].children"` into segments.
fn parse_path_segments(path: &str) -> Vec<PathSegment> {
    let mut segments = Vec::new();
    for part in path.split('.') {
        if part.is_empty() {
            continue;
        }
        // Check for array index notation: "items[0]"
        if let Some(bracket_pos) = part.find('[') {
            let key = &part[..bracket_pos];
            if !key.is_empty() {
                segments.push(PathSegment::Key(key.to_string()));
            }
            // Extract all indices like [0][1]
            let rest = &part[bracket_pos..];
            let mut i = 0;
            let bytes = rest.as_bytes();
            while i < bytes.len() {
                if bytes[i] == b'[' {
                    if let Some(close) = rest[i..].find(']') {
                        let idx_str = &rest[i + 1..i + close];
                        if let Ok(idx) = idx_str.parse::<usize>() {
                            segments.push(PathSegment::Index(idx));
                        }
                        i += close + 1;
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else {
            segments.push(PathSegment::Key(part.to_string()));
        }
    }
    segments
}

// ---------------------------------------------------------------------------
// MapStep
// ---------------------------------------------------------------------------

pub struct MapStep;

#[async_trait]
impl StepHandler for MapStep {
    fn name(&self) -> &'static str {
        "map"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        // Auto-wrap single objects into an array
        let owned_arr;
        let arr = match data.as_array() {
            Some(a) => a,
            None if data.is_object() => {
                owned_arr = vec![data.clone()];
                &owned_arr
            }
            _ => return Err(CliError::pipeline("map: data must be an array or object")),
        };

        let template = params;
        let mut results = Vec::with_capacity(arr.len());

        for (i, item) in arr.iter().enumerate() {
            let ctx = TemplateContext {
                args: args.clone(),
                data: data.clone(),
                item: item.clone(),
                index: i,
            };
            let rendered = render_template(template, &ctx)?;
            results.push(rendered);
        }

        Ok(Value::Array(results))
    }
}

// ---------------------------------------------------------------------------
// FilterStep
// ---------------------------------------------------------------------------

pub struct FilterStep;

fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0
            } else {
                true
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(_) => true,
        Value::Object(_) => true,
    }
}

#[async_trait]
impl StepHandler for FilterStep {
    fn name(&self) -> &'static str {
        "filter"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let arr = data
            .as_array()
            .ok_or_else(|| CliError::pipeline("filter: data must be an array"))?;

        let condition = params
            .as_str()
            .ok_or_else(|| CliError::pipeline("filter: params must be a condition string"))?;

        // Wrap in ${{ }} if not already wrapped
        let template = if condition.contains("${{") {
            condition.to_string()
        } else {
            format!("${{{{ {} }}}}", condition)
        };

        let mut results = Vec::new();
        for (i, item) in arr.iter().enumerate() {
            let ctx = TemplateContext {
                args: args.clone(),
                data: data.clone(),
                item: item.clone(),
                index: i,
            };
            let val = render_template_str(&template, &ctx)?;
            if is_truthy(&val) {
                results.push(item.clone());
            }
        }

        Ok(Value::Array(results))
    }
}

// ---------------------------------------------------------------------------
// SortStep
// ---------------------------------------------------------------------------

pub struct SortStep;

#[async_trait]
impl StepHandler for SortStep {
    fn name(&self) -> &'static str {
        "sort"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        _args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let mut arr = data
            .as_array()
            .ok_or_else(|| CliError::pipeline("sort: data must be an array"))?
            .clone();

        let (field, descending) = match params {
            Value::String(s) => (s.clone(), false),
            Value::Object(obj) => {
                let by = obj
                    .get("by")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CliError::pipeline("sort: missing 'by' field"))?
                    .to_string();
                let desc = obj
                    .get("order")
                    .and_then(|v| v.as_str())
                    .map(|o| o == "desc")
                    .unwrap_or(false);
                (by, desc)
            }
            _ => return Err(CliError::pipeline("sort: invalid params")),
        };

        arr.sort_by(|a, b| {
            let va = a.get(&field);
            let vb = b.get(&field);
            let cmp = compare_values(va, vb);
            if descending {
                cmp.reverse()
            } else {
                cmp
            }
        });

        Ok(Value::Array(arr))
    }
}

fn compare_values(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(Value::Number(na)), Some(Value::Number(nb))) => {
            let fa = na.as_f64().unwrap_or(0.0);
            let fb = nb.as_f64().unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Some(Value::String(sa)), Some(Value::String(sb))) => sa.cmp(sb),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

// ---------------------------------------------------------------------------
// LimitStep
// ---------------------------------------------------------------------------

pub struct LimitStep;

#[async_trait]
impl StepHandler for LimitStep {
    fn name(&self) -> &'static str {
        "limit"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        // Auto-wrap single objects into an array
        let owned_arr;
        let arr = match data.as_array() {
            Some(a) => a,
            None if data.is_object() => {
                owned_arr = vec![data.clone()];
                &owned_arr
            }
            _ => return Err(CliError::pipeline("limit: data must be an array or object")),
        };

        let n = match params {
            Value::Number(n) => n
                .as_u64()
                .ok_or_else(|| CliError::pipeline("limit: invalid number"))?
                as usize,
            Value::String(s) => {
                let ctx = TemplateContext {
                    args: args.clone(),
                    data: data.clone(),
                    item: Value::Null,
                    index: 0,
                };
                let val = render_template_str(s, &ctx)?;
                val.as_u64()
                    .or_else(|| val.as_str().and_then(|s| s.parse::<u64>().ok()))
                    .ok_or_else(|| {
                        CliError::pipeline("limit: template did not resolve to a number")
                    })? as usize
            }
            _ => {
                return Err(CliError::pipeline(
                    "limit: params must be a number or template string",
                ))
            }
        };

        let truncated: Vec<Value> = arr.iter().take(n).cloned().collect();
        Ok(Value::Array(truncated))
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register_transform_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(SelectStep));
    registry.register(Arc::new(MapStep));
    registry.register(Arc::new(FilterStep));
    registry.register(Arc::new(SortStep));
    registry.register(Arc::new(LimitStep));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_args() -> HashMap<String, Value> {
        HashMap::new()
    }

    #[tokio::test]
    async fn test_select_step() {
        let step = SelectStep;
        let data = json!({"data": {"list": [1, 2, 3]}});
        let result = step
            .execute(None, &json!("data.list"), &data, &empty_args())
            .await
            .unwrap();
        assert_eq!(result, json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn test_map_step() {
        let step = MapStep;
        let data = json!([{"title": "Hello", "score": 42}, {"title": "World", "score": 10}]);
        let params = json!({"rank": "${{ index + 1 }}", "title": "${{ item.title }}", "score": "${{ item.score }}"});
        let result = step
            .execute(None, &params, &data, &empty_args())
            .await
            .unwrap();
        assert_eq!(
            result,
            json!([
                {"rank": 1, "title": "Hello", "score": 42},
                {"rank": 2, "title": "World", "score": 10}
            ])
        );
    }

    #[tokio::test]
    async fn test_filter_step() {
        let step = FilterStep;
        let data = json!([
            {"title": "Good", "deleted": false},
            {"title": "Bad", "deleted": true},
            {"title": "Also Good", "deleted": false}
        ]);
        let params = json!("!item.deleted");
        let result = step
            .execute(None, &params, &data, &empty_args())
            .await
            .unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["title"], "Good");
    }

    #[tokio::test]
    async fn test_sort_step_desc() {
        let step = SortStep;
        let data = json!([{"score": 10}, {"score": 42}, {"score": 5}]);
        let params = json!({"by": "score", "order": "desc"});
        let result = step
            .execute(None, &params, &data, &empty_args())
            .await
            .unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["score"], 42);
        assert_eq!(arr[1]["score"], 10);
        assert_eq!(arr[2]["score"], 5);
    }

    #[tokio::test]
    async fn test_limit_step() {
        let step = LimitStep;
        let data = json!([1, 2, 3, 4, 5]);
        let result = step
            .execute(None, &json!(3), &data, &empty_args())
            .await
            .unwrap();
        assert_eq!(result, json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn test_limit_step_with_template() {
        let step = LimitStep;
        let data = json!([1, 2, 3, 4, 5]);
        let mut args = HashMap::new();
        args.insert("limit".to_string(), json!(2));
        let result = step
            .execute(None, &json!("${{ args.limit }}"), &data, &args)
            .await
            .unwrap();
        assert_eq!(result, json!([1, 2]));
    }

    #[tokio::test]
    async fn test_register_all() {
        let mut registry = StepRegistry::new();
        register_transform_steps(&mut registry);
        assert!(registry.get("select").is_some());
        assert!(registry.get("map").is_some());
        assert!(registry.get("filter").is_some());
        assert!(registry.get("sort").is_some());
        assert!(registry.get("limit").is_some());
    }
}
