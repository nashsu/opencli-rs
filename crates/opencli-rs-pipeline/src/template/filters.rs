use serde_json::Value;

use opencli_rs_core::CliError;

pub fn apply_filter(name: &str, input: Value, args: &[Value]) -> Result<Value, CliError> {
    match name {
        "default" => filter_default(input, args),
        "join" => filter_join(input, args),
        "upper" => filter_upper(input),
        "lower" => filter_lower(input),
        "trim" => filter_trim(input),
        "truncate" => filter_truncate(input, args),
        "replace" => filter_replace(input, args),
        "keys" => filter_keys(input),
        "length" => filter_length(input),
        "first" => filter_first(input),
        "last" => filter_last(input),
        "json" => filter_json(input),
        "slugify" => filter_slugify(input),
        "sanitize" => filter_sanitize(input),
        "ext" => filter_ext(input),
        "basename" => filter_basename(input),
        "urlencode" => filter_urlencode(input),
        "urldecode" => filter_urldecode(input),
        "abs" => filter_abs(input),
        "round" => filter_round(input),
        "ceil" => filter_ceil(input),
        "floor" => filter_floor(input),
        "string" | "str" => filter_string(input),
        "int" => filter_int(input),
        "float" => filter_float(input),
        "reverse" => filter_reverse(input),
        "unique" => filter_unique(input),
        "split" => filter_split(input, args),
        _ => Err(CliError::pipeline(format!("Unknown filter: {name}"))),
    }
}

fn filter_default(input: Value, args: &[Value]) -> Result<Value, CliError> {
    let default_val = args.first().cloned().unwrap_or(Value::Null);
    match &input {
        Value::Null => Ok(default_val),
        Value::String(s) if s.is_empty() => Ok(default_val),
        _ => Ok(input),
    }
}

fn filter_join(input: Value, args: &[Value]) -> Result<Value, CliError> {
    let sep = args.first().and_then(|v| v.as_str()).unwrap_or(",");
    match input {
        Value::Array(arr) => {
            let parts: Vec<String> = arr
                .into_iter()
                .map(|v| match v {
                    Value::String(s) => s,
                    other => other.to_string(),
                })
                .collect();
            Ok(Value::String(parts.join(sep)))
        }
        _ => Ok(input),
    }
}

fn filter_upper(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::String(s) => Value::String(s.to_uppercase()),
        other => other,
    })
}

fn filter_lower(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::String(s) => Value::String(s.to_lowercase()),
        other => other,
    })
}

fn filter_trim(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::String(s) => Value::String(s.trim().to_string()),
        other => other,
    })
}

fn filter_truncate(input: Value, args: &[Value]) -> Result<Value, CliError> {
    let n = args.first().and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    Ok(match input {
        Value::String(s) => {
            if s.chars().count() > n {
                let truncated: String = s.chars().take(n).collect();
                Value::String(format!("{truncated}..."))
            } else {
                Value::String(s)
            }
        }
        other => other,
    })
}

fn filter_replace(input: Value, args: &[Value]) -> Result<Value, CliError> {
    let old = args.first().and_then(|v| v.as_str()).unwrap_or("");
    let new = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
    Ok(match input {
        Value::String(s) => Value::String(s.replace(old, new)),
        other => other,
    })
}

fn filter_keys(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::Object(map) => Value::Array(map.keys().map(|k| Value::String(k.clone())).collect()),
        _ => Value::Array(vec![]),
    })
}

fn filter_length(input: Value) -> Result<Value, CliError> {
    let len = match &input {
        Value::String(s) => s.len(),
        Value::Array(arr) => arr.len(),
        Value::Object(map) => map.len(),
        _ => 0,
    };
    Ok(Value::Number(serde_json::Number::from(len)))
}

fn filter_first(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::Array(arr) => arr.into_iter().next().unwrap_or(Value::Null),
        _ => Value::Null,
    })
}

fn filter_last(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::Array(arr) => arr.into_iter().last().unwrap_or(Value::Null),
        _ => Value::Null,
    })
}

fn filter_json(input: Value) -> Result<Value, CliError> {
    Ok(Value::String(
        serde_json::to_string(&input).unwrap_or_default(),
    ))
}

fn filter_slugify(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::String(s) => {
            let slug: String = s
                .to_lowercase()
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() {
                        c
                    } else if c == ' ' || c == '_' {
                        '-'
                    } else {
                        '\0'
                    }
                })
                .filter(|c| *c != '\0')
                .collect();
            // Collapse multiple hyphens
            let mut result = String::new();
            let mut last_hyphen = false;
            for c in slug.chars() {
                if c == '-' {
                    if !last_hyphen {
                        result.push(c);
                    }
                    last_hyphen = true;
                } else {
                    result.push(c);
                    last_hyphen = false;
                }
            }
            Value::String(result.trim_matches('-').to_string())
        }
        other => other,
    })
}

fn filter_sanitize(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::String(s) => {
            // Strip HTML tags
            let mut result = String::new();
            let mut in_tag = false;
            for c in s.chars() {
                if c == '<' {
                    in_tag = true;
                } else if c == '>' {
                    in_tag = false;
                } else if !in_tag {
                    result.push(c);
                }
            }
            Value::String(result)
        }
        other => other,
    })
}

fn filter_ext(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::String(s) => {
            if let Some(pos) = s.rfind('.') {
                Value::String(s[pos..].to_string())
            } else {
                Value::String(String::new())
            }
        }
        other => other,
    })
}

fn filter_basename(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::String(s) => {
            let name = s.rsplit('/').next().unwrap_or(&s);
            Value::String(name.to_string())
        }
        other => other,
    })
}

fn filter_urlencode(input: Value) -> Result<Value, CliError> {
    let s = match &input {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    // Percent-encode all non-unreserved characters per RFC 3986
    let encoded: String = s
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                format!("{}", b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect();
    Ok(Value::String(encoded))
}

fn filter_urldecode(input: Value) -> Result<Value, CliError> {
    let s = match &input {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                result.push(val);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            result.push(b' ');
        } else {
            result.push(bytes[i]);
        }
        i += 1;
    }
    Ok(Value::String(String::from_utf8_lossy(&result).to_string()))
}

fn filter_abs(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.abs().into())
            } else if let Some(f) = n.as_f64() {
                Value::Number(serde_json::Number::from_f64(f.abs()).unwrap_or(n))
            } else {
                Value::Number(n)
            }
        }
        other => other,
    })
}

fn filter_round(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Value::Number(serde_json::Number::from_f64(f.round()).unwrap_or(n))
            } else {
                Value::Number(n)
            }
        }
        other => other,
    })
}

fn filter_ceil(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Value::Number(serde_json::Number::from_f64(f.ceil()).unwrap_or(n))
            } else {
                Value::Number(n)
            }
        }
        other => other,
    })
}

fn filter_floor(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Value::Number(serde_json::Number::from_f64(f.floor()).unwrap_or(n))
            } else {
                Value::Number(n)
            }
        }
        other => other,
    })
}

fn filter_string(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::String(_) => input,
        Value::Null => Value::String(String::new()),
        other => Value::String(other.to_string()),
    })
}

fn filter_int(input: Value) -> Result<Value, CliError> {
    Ok(match &input {
        Value::Number(n) => Value::Number(n.as_i64().unwrap_or(0).into()),
        Value::String(s) => {
            let n: i64 = s.parse().unwrap_or(0);
            Value::Number(n.into())
        }
        Value::Bool(b) => Value::Number(if *b { 1 } else { 0 }.into()),
        _ => Value::Number(0.into()),
    })
}

fn filter_float(input: Value) -> Result<Value, CliError> {
    Ok(match &input {
        Value::Number(_) => input.clone(),
        Value::String(s) => {
            let f: f64 = s.parse().unwrap_or(0.0);
            Value::Number(serde_json::Number::from_f64(f).unwrap_or(0.into()))
        }
        _ => Value::Number(serde_json::Number::from_f64(0.0).unwrap_or(0.into())),
    })
}

fn filter_reverse(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::Array(mut arr) => {
            arr.reverse();
            Value::Array(arr)
        }
        Value::String(s) => Value::String(s.chars().rev().collect()),
        other => other,
    })
}

fn filter_unique(input: Value) -> Result<Value, CliError> {
    Ok(match input {
        Value::Array(arr) => {
            let mut seen = Vec::new();
            let mut result = Vec::new();
            for item in arr {
                let key = item.to_string();
                if !seen.contains(&key) {
                    seen.push(key);
                    result.push(item);
                }
            }
            Value::Array(result)
        }
        other => other,
    })
}

fn filter_split(input: Value, args: &[Value]) -> Result<Value, CliError> {
    let sep = args.first().and_then(|v| v.as_str()).unwrap_or(",");
    Ok(match input {
        Value::String(s) => {
            Value::Array(s.split(sep).map(|p| Value::String(p.to_string())).collect())
        }
        other => other,
    })
}
