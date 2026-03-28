mod evaluator;
mod filters;
mod parser;

pub use evaluator::TemplateContext;

use opencli_rs_core::CliError;
use serde_json::Value;

use evaluator::evaluate;
use parser::parse_expression;

/// Render a Value that may contain template strings.
/// If Value is a String containing `${{ }}`, evaluate it.
/// If Value is an Object/Array, recursively render all string values.
pub fn render_template(value: &Value, ctx: &TemplateContext) -> Result<Value, CliError> {
    match value {
        Value::String(s) => render_template_str(s, ctx),
        Value::Array(arr) => {
            let rendered: Result<Vec<Value>, _> =
                arr.iter().map(|v| render_template(v, ctx)).collect();
            Ok(Value::Array(rendered?))
        }
        Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (k, v) in map {
                result.insert(k.clone(), render_template(v, ctx)?);
            }
            Ok(Value::Object(result))
        }
        // Numbers, bools, null pass through unchanged
        other => Ok(other.clone()),
    }
}

/// Render a single template string.
/// - If the entire string is `${{ expr }}`, returns typed Value.
/// - If it contains `${{ expr }}` mixed with text, returns String.
/// - If no template markers, returns the string as-is.
pub fn render_template_str(template: &str, ctx: &TemplateContext) -> Result<Value, CliError> {
    let markers = find_template_markers(template);

    if markers.is_empty() {
        // No template expressions
        return Ok(Value::String(template.to_string()));
    }

    // Check if the entire string is a single expression
    if markers.len() == 1 {
        let (start, end, expr_str) = &markers[0];
        if *start == 0 && *end == template.len() {
            // Full expression mode: return typed value
            let ast = parse_expression(expr_str.trim())?;
            return evaluate(&ast, ctx);
        }
    }

    // Partial interpolation: build string
    let mut result = String::new();
    let mut last_end = 0;

    for (start, end, expr_str) in &markers {
        result.push_str(&template[last_end..*start]);
        let ast = parse_expression(expr_str.trim())?;
        let val = evaluate(&ast, ctx)?;
        result.push_str(&value_to_string(&val));
        last_end = *end;
    }
    result.push_str(&template[last_end..]);

    Ok(Value::String(result))
}

/// Find all `${{ ... }}` markers in a string, returning (start, end, inner_expr).
fn find_template_markers(s: &str) -> Vec<(usize, usize, String)> {
    let mut markers = Vec::new();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if i + 3 < len && bytes[i] == b'$' && bytes[i + 1] == b'{' && bytes[i + 2] == b'{' {
            let start = i;
            let expr_start = i + 3;
            // Find closing }}
            let mut j = expr_start;
            let mut depth = 0;
            while j + 1 < len {
                if bytes[j] == b'{' {
                    depth += 1;
                } else if bytes[j] == b'}' && bytes[j + 1] == b'}' {
                    if depth == 0 {
                        let expr = s[expr_start..j].to_string();
                        let end = j + 2;
                        markers.push((start, end, expr));
                        i = end;
                        break;
                    }
                    depth -= 1;
                }
                j += 1;
            }
            if j + 1 >= len {
                // Unclosed marker, skip
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    markers
}

fn value_to_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => serde_json::to_string(val).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_ctx() -> TemplateContext {
        let mut args = HashMap::new();
        args.insert("limit".to_string(), Value::Number(20.into()));

        let data = serde_json::json!([
            {"name": "first_item"},
            {"name": "second_item"},
        ]);

        let item = serde_json::json!({
            "id": 42,
            "title": "Hello World",
            "subtitle": null,
            "score": 15,
            "active": true,
            "deleted": false,
            "author": {
                "name": "Alice"
            },
            "tags": ["rust", "cli"],
            "path": "/home/user/docs/readme.md",
            "html": "<b>bold</b> text",
            "name": "  Hello World  "
        });

        TemplateContext {
            args,
            data,
            item,
            index: 0,
        }
    }

    #[test]
    fn test_simple_variable() {
        let ctx = make_ctx();
        let result = render_template_str("${{ args.limit }}", &ctx).unwrap();
        assert_eq!(result, Value::Number(20.into()));
    }

    #[test]
    fn test_nested_path() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.author.name }}", &ctx).unwrap();
        assert_eq!(result, Value::String("Alice".to_string()));
    }

    #[test]
    fn test_arithmetic() {
        let ctx = make_ctx();
        let result = render_template_str("${{ index + 1 }}", &ctx).unwrap();
        assert_eq!(result, Value::Number(1.into()));
    }

    #[test]
    fn test_comparison() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.score > 10 }}", &ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_pipe_filter() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.title | truncate(5) }}", &ctx).unwrap();
        assert_eq!(result, Value::String("Hello...".to_string()));
    }

    #[test]
    fn test_fallback() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.subtitle || \"N/A\" }}", &ctx).unwrap();
        assert_eq!(result, Value::String("N/A".to_string()));
    }

    #[test]
    fn test_partial_interpolation() {
        let ctx = make_ctx();
        let result = render_template_str("https://api.com/${{ item.id }}.json", &ctx).unwrap();
        assert_eq!(result, Value::String("https://api.com/42.json".to_string()));
    }

    #[test]
    fn test_ternary() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.active ? \"yes\" : \"no\" }}", &ctx).unwrap();
        assert_eq!(result, Value::String("yes".to_string()));
    }

    #[test]
    fn test_filter_chain() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.name | lower | trim }}", &ctx).unwrap();
        assert_eq!(result, Value::String("hello world".to_string()));
    }

    #[test]
    fn test_math_min() {
        let ctx = make_ctx();
        let result = render_template_str("${{ Math.min(args.limit + 10, 50) }}", &ctx).unwrap();
        assert_eq!(result, Value::Number(30.into()));
    }

    #[test]
    fn test_array_index() {
        let ctx = make_ctx();
        let result = render_template_str("${{ data[0].name }}", &ctx).unwrap();
        assert_eq!(result, Value::String("first_item".to_string()));
    }

    #[test]
    fn test_logical_and() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.title && !item.deleted }}", &ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_render_template_value_object() {
        let ctx = make_ctx();
        let input = serde_json::json!({
            "rank": "${{ index + 1 }}",
            "title": "${{ item.title }}"
        });
        let result = render_template(&input, &ctx).unwrap();
        let obj = result.as_object().unwrap();
        assert_eq!(obj.get("rank").unwrap(), &Value::Number(1.into()));
        assert_eq!(
            obj.get("title").unwrap(),
            &Value::String("Hello World".to_string())
        );
    }

    #[test]
    fn test_no_template() {
        let ctx = make_ctx();
        let result = render_template_str("plain text", &ctx).unwrap();
        assert_eq!(result, Value::String("plain text".to_string()));
    }

    #[test]
    fn test_filter_upper() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.title | upper }}", &ctx).unwrap();
        assert_eq!(result, Value::String("HELLO WORLD".to_string()));
    }

    #[test]
    fn test_filter_join() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.tags | join(\", \") }}", &ctx).unwrap();
        assert_eq!(result, Value::String("rust, cli".to_string()));
    }

    #[test]
    fn test_filter_length() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.tags | length }}", &ctx).unwrap();
        assert_eq!(result, Value::Number(2.into()));
    }

    #[test]
    fn test_filter_keys() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.author | keys }}", &ctx).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0], Value::String("name".to_string()));
    }

    #[test]
    fn test_filter_first_last() {
        let ctx = make_ctx();
        let first = render_template_str("${{ item.tags | first }}", &ctx).unwrap();
        assert_eq!(first, Value::String("rust".to_string()));
        let last = render_template_str("${{ item.tags | last }}", &ctx).unwrap();
        assert_eq!(last, Value::String("cli".to_string()));
    }

    #[test]
    fn test_filter_json() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.author | json }}", &ctx).unwrap();
        assert_eq!(result, Value::String("{\"name\":\"Alice\"}".to_string()));
    }

    #[test]
    fn test_filter_slugify() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.title | slugify }}", &ctx).unwrap();
        assert_eq!(result, Value::String("hello-world".to_string()));
    }

    #[test]
    fn test_filter_sanitize() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.html | sanitize }}", &ctx).unwrap();
        assert_eq!(result, Value::String("bold text".to_string()));
    }

    #[test]
    fn test_filter_ext() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.path | ext }}", &ctx).unwrap();
        assert_eq!(result, Value::String(".md".to_string()));
    }

    #[test]
    fn test_filter_basename() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.path | basename }}", &ctx).unwrap();
        assert_eq!(result, Value::String("readme.md".to_string()));
    }

    #[test]
    fn test_filter_replace() {
        let ctx = make_ctx();
        let result =
            render_template_str("${{ item.title | replace(\"World\", \"Rust\") }}", &ctx).unwrap();
        assert_eq!(result, Value::String("Hello Rust".to_string()));
    }

    #[test]
    fn test_filter_default() {
        let ctx = make_ctx();
        let result =
            render_template_str("${{ item.subtitle | default(\"fallback\") }}", &ctx).unwrap();
        assert_eq!(result, Value::String("fallback".to_string()));
    }

    #[test]
    fn test_math_max() {
        let ctx = make_ctx();
        let result = render_template_str("${{ Math.max(5, 10) }}", &ctx).unwrap();
        assert_eq!(result, Value::Number(10.into()));
    }

    #[test]
    fn test_equality() {
        let ctx = make_ctx();
        let result = render_template_str("${{ item.id == 42 }}", &ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_modulo() {
        let ctx = make_ctx();
        let result = render_template_str("${{ index % 2 }}", &ctx).unwrap();
        assert_eq!(result, Value::Number(0.into()));
    }

    #[test]
    fn test_parentheses() {
        let ctx = make_ctx();
        let result = render_template_str("${{ (index + 1) * 2 }}", &ctx).unwrap();
        assert_eq!(result, Value::Number(2.into()));
    }

    #[test]
    fn test_string_literal() {
        let ctx = make_ctx();
        let result = render_template_str("${{ 'hello' }}", &ctx).unwrap();
        assert_eq!(result, Value::String("hello".to_string()));
    }

    #[test]
    fn test_boolean_literal() {
        let ctx = make_ctx();
        let result = render_template_str("${{ true }}", &ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_null_literal() {
        let ctx = make_ctx();
        let result = render_template_str("${{ null }}", &ctx).unwrap();
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn test_multiple_interpolations() {
        let ctx = make_ctx();
        let result =
            render_template_str("${{ item.title }} by ${{ item.author.name }}", &ctx).unwrap();
        assert_eq!(result, Value::String("Hello World by Alice".to_string()));
    }

    #[test]
    fn test_render_template_array() {
        let ctx = make_ctx();
        let input = serde_json::json!(["${{ item.title }}", "${{ index }}"]);
        let result = render_template(&input, &ctx).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0], Value::String("Hello World".to_string()));
        assert_eq!(arr[1], Value::Number(0.into()));
    }
}
