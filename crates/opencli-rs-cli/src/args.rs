use opencli_rs_core::{ArgDef, ArgType, CliError};
use serde_json::Value;
use std::collections::HashMap;

/// Coerce and validate command arguments against their definitions.
/// - Converts string inputs to typed values (str->int, str->bool, etc.)
/// - Checks required arguments are present
/// - Validates choices
/// - Fills in defaults
pub fn coerce_and_validate_args(
    defs: &[ArgDef],
    raw: &HashMap<String, String>,
) -> Result<HashMap<String, Value>, CliError> {
    let mut result = HashMap::new();

    for def in defs {
        match raw.get(&def.name) {
            Some(raw_val) => {
                let val = coerce_value(raw_val, &def.arg_type, &def.name)?;
                if let Some(choices) = &def.choices {
                    let s = raw_val.to_string();
                    if !choices.contains(&s) {
                        return Err(CliError::argument(format!(
                            "'{}' must be one of: {}",
                            def.name,
                            choices.join(", ")
                        )));
                    }
                }
                result.insert(def.name.clone(), val);
            }
            None => {
                if def.required {
                    return Err(CliError::argument(format!(
                        "Missing required argument: {}",
                        def.name
                    )));
                }
                if let Some(default) = &def.default {
                    result.insert(def.name.clone(), default.clone());
                }
            }
        }
    }

    Ok(result)
}

fn coerce_value(raw: &str, arg_type: &ArgType, name: &str) -> Result<Value, CliError> {
    match arg_type {
        ArgType::Str => Ok(Value::String(raw.to_string())),
        ArgType::Int => raw
            .parse::<i64>()
            .map(|n| Value::Number(n.into()))
            .map_err(|_| {
                CliError::argument(format!("'{}' expects an integer, got: {}", name, raw))
            }),
        ArgType::Number => raw
            .parse::<f64>()
            .map(|n| Value::Number(serde_json::Number::from_f64(n).unwrap_or(0.into())))
            .map_err(|_| CliError::argument(format!("'{}' expects a number, got: {}", name, raw))),
        ArgType::Bool | ArgType::Boolean => match raw.to_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(Value::Bool(true)),
            "false" | "0" | "no" => Ok(Value::Bool(false)),
            _ => Err(CliError::argument(format!(
                "'{}' expects a boolean, got: {}",
                name, raw
            ))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencli_rs_core::ArgDef;

    fn make_arg(name: &str, arg_type: ArgType, required: bool) -> ArgDef {
        ArgDef {
            name: name.to_string(),
            arg_type,
            required,
            positional: false,
            description: None,
            choices: None,
            default: None,
        }
    }

    #[test]
    fn test_int_coercion() {
        let defs = vec![make_arg("count", ArgType::Int, false)];
        let mut raw = HashMap::new();
        raw.insert("count".to_string(), "42".to_string());

        let result = coerce_and_validate_args(&defs, &raw).unwrap();
        assert_eq!(result.get("count"), Some(&Value::Number(42.into())));
    }

    #[test]
    fn test_bool_coercion() {
        let defs = vec![make_arg("flag", ArgType::Bool, false)];
        let mut raw = HashMap::new();
        raw.insert("flag".to_string(), "true".to_string());

        let result = coerce_and_validate_args(&defs, &raw).unwrap();
        assert_eq!(result.get("flag"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_required_missing() {
        let defs = vec![make_arg("name", ArgType::Str, true)];
        let raw = HashMap::new();

        let err = coerce_and_validate_args(&defs, &raw).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Missing required argument: name"));
    }

    #[test]
    fn test_choices_validation() {
        let mut def = make_arg("color", ArgType::Str, false);
        def.choices = Some(vec!["red".to_string(), "blue".to_string()]);

        let defs = vec![def];
        let mut raw = HashMap::new();
        raw.insert("color".to_string(), "green".to_string());

        let err = coerce_and_validate_args(&defs, &raw).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("must be one of"));
    }

    #[test]
    fn test_default_filling() {
        let mut def = make_arg("limit", ArgType::Int, false);
        def.default = Some(Value::Number(10.into()));

        let defs = vec![def];
        let raw = HashMap::new();

        let result = coerce_and_validate_args(&defs, &raw).unwrap();
        assert_eq!(result.get("limit"), Some(&Value::Number(10.into())));
    }
}
