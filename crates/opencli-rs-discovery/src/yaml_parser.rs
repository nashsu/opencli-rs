use opencli_rs_core::{ArgDef, ArgType, CliCommand, CliError, NavigateBefore, Strategy};
use serde_json::Value;

/// Parse a YAML adapter file content into a CliCommand.
pub fn parse_yaml_adapter(content: &str) -> Result<CliCommand, CliError> {
    let raw: Value = serde_yaml::from_str(content).map_err(|e| CliError::AdapterLoad {
        message: format!("Failed to parse YAML: {}", e),
        suggestions: vec![],
        source: None,
    })?;

    let site = raw
        .get("site")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::AdapterLoad {
            message: "Missing 'site' field".into(),
            suggestions: vec![],
            source: None,
        })?
        .to_string();

    let name = raw
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::AdapterLoad {
            message: "Missing 'name' field".into(),
            suggestions: vec![],
            source: None,
        })?
        .to_string();

    // Parse strategy (default: public)
    let strategy = match raw.get("strategy").and_then(|v| v.as_str()) {
        Some(s) => serde_json::from_value(Value::String(s.to_string())).unwrap_or(Strategy::Public),
        None => Strategy::Public,
    };

    // Parse args — in YAML they're a map: { limit: { type: int, default: 20 } }
    let args = parse_args(&raw)?;

    // Parse columns
    let columns = raw
        .get("columns")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Pipeline is stored as-is (Vec<Value>)
    let pipeline = raw.get("pipeline").and_then(|v| v.as_array()).cloned();

    Ok(CliCommand {
        site,
        name,
        description: raw
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        domain: raw.get("domain").and_then(|v| v.as_str()).map(String::from),
        strategy,
        browser: raw
            .get("browser")
            .and_then(|v| v.as_bool())
            .unwrap_or(strategy.requires_browser()),
        args,
        columns,
        pipeline,
        func: None,
        timeout_seconds: raw.get("timeoutSeconds").and_then(|v| v.as_u64()),
        navigate_before: NavigateBefore::default(),
    })
}

/// Parse args from YAML map format to Vec<ArgDef>
fn parse_args(raw: &Value) -> Result<Vec<ArgDef>, CliError> {
    let args_val = match raw.get("args") {
        Some(v) if v.is_object() => v,
        Some(v) if v.is_array() && v.as_array().unwrap().is_empty() => return Ok(vec![]),
        _ => return Ok(vec![]),
    };

    let map = args_val.as_object().unwrap();
    let mut result = vec![];

    for (name, def) in map {
        let arg_type = match def.get("type").and_then(|v| v.as_str()) {
            Some("int") => ArgType::Int,
            Some("number") => ArgType::Number,
            Some("bool") => ArgType::Bool,
            Some("boolean") => ArgType::Boolean,
            _ => ArgType::Str,
        };

        result.push(ArgDef {
            name: name.clone(),
            arg_type,
            required: def
                .get("required")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            positional: def
                .get("positional")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            description: def
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            choices: def.get("choices").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            }),
            default: def.get("default").cloned(),
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_adapter() {
        let yaml = r#"
site: hackernews
name: top
description: Top stories
strategy: public
browser: false
args:
  limit:
    type: int
    default: 20
    description: Number of items
columns: [rank, title, score, author]
pipeline:
  - fetch: https://hacker-news.firebaseio.com/v0/topstories.json
  - limit: "${{ args.limit }}"
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.site, "hackernews");
        assert_eq!(cmd.name, "top");
        assert_eq!(cmd.strategy, Strategy::Public);
        assert!(!cmd.browser);
        assert_eq!(cmd.args.len(), 1);
        assert_eq!(cmd.args[0].name, "limit");
        assert_eq!(cmd.args[0].arg_type, ArgType::Int);
        assert_eq!(cmd.columns, vec!["rank", "title", "score", "author"]);
        assert!(cmd.pipeline.is_some());
        assert_eq!(cmd.pipeline.unwrap().len(), 2);
    }

    #[test]
    fn test_parse_cookie_strategy() {
        let yaml = r#"
site: bilibili
name: hot
description: Hot videos
strategy: cookie
domain: www.bilibili.com
"#;
        let cmd = parse_yaml_adapter(yaml).unwrap();
        assert_eq!(cmd.strategy, Strategy::Cookie);
        assert!(cmd.browser); // cookie strategy implies browser
        assert_eq!(cmd.domain, Some("www.bilibili.com".to_string()));
    }

    #[test]
    fn test_parse_missing_site_errors() {
        let yaml = "name: test\n";
        assert!(parse_yaml_adapter(yaml).is_err());
    }
}
