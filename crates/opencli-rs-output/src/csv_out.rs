use serde_json::Value;

/// Extract column names from data, respecting optional column selection.
fn resolve_columns(data: &Value, columns: Option<&[String]>) -> Vec<String> {
    if let Some(cols) = columns {
        return cols.to_vec();
    }
    match data {
        Value::Array(arr) => {
            if let Some(Value::Object(obj)) = arr.first() {
                obj.keys().cloned().collect()
            } else {
                vec![]
            }
        }
        Value::Object(obj) => obj.keys().cloned().collect(),
        _ => vec![],
    }
}

fn value_to_field(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Render data as RFC 4180 CSV.
pub fn render_csv(data: &Value, columns: Option<&[String]>) -> String {
    match data {
        Value::Null => String::new(),
        Value::Array(arr) if arr.is_empty() => String::new(),
        Value::Array(arr) => {
            let cols = resolve_columns(data, columns);
            if cols.is_empty() {
                // Array of scalars
                let mut wtr = csv::WriterBuilder::new().from_writer(vec![]);
                wtr.write_record(["value"]).ok();
                for item in arr {
                    wtr.write_record(&[value_to_field(item)]).ok();
                }
                wtr.flush().ok();
                String::from_utf8(wtr.into_inner().unwrap_or_default()).unwrap_or_default()
            } else {
                let mut wtr = csv::WriterBuilder::new().from_writer(vec![]);
                wtr.write_record(&cols).ok();
                for item in arr {
                    let row: Vec<String> = cols
                        .iter()
                        .map(|col| value_to_field(item.get(col).unwrap_or(&Value::Null)))
                        .collect();
                    wtr.write_record(&row).ok();
                }
                wtr.flush().ok();
                String::from_utf8(wtr.into_inner().unwrap_or_default()).unwrap_or_default()
            }
        }
        Value::Object(obj) => {
            let cols = resolve_columns(data, columns);
            let mut wtr = csv::WriterBuilder::new().from_writer(vec![]);
            wtr.write_record(["key", "value"]).ok();
            for key in &cols {
                let v = obj.get(key).unwrap_or(&Value::Null);
                wtr.write_record([key.as_str(), &value_to_field(v)]).ok();
            }
            wtr.flush().ok();
            String::from_utf8(wtr.into_inner().unwrap_or_default()).unwrap_or_default()
        }
        scalar => value_to_field(scalar),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_array_of_objects() {
        let data = json!([{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]);
        let out = render_csv(&data, None);
        assert!(out.contains("name"));
        assert!(out.contains("Alice"));
        assert!(out.contains("Bob"));
    }

    #[test]
    fn test_single_object() {
        let data = json!({"name": "Alice", "age": 30});
        let out = render_csv(&data, None);
        assert!(out.contains("key,value"));
        assert!(out.contains("Alice"));
    }

    #[test]
    fn test_empty_array() {
        let data = json!([]);
        let out = render_csv(&data, None);
        assert!(out.is_empty());
    }

    #[test]
    fn test_column_selection() {
        let data = json!([{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]);
        let cols = vec!["name".to_string()];
        let out = render_csv(&data, Some(&cols));
        assert!(out.contains("name"));
        assert!(out.contains("Alice"));
        assert!(!out.contains("age"));
    }
}
