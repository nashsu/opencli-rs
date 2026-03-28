use comfy_table::{Cell, Table};
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

/// Convert a JSON value to a display string, stringifying nested structures.
fn value_to_cell(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        // Nested object/array → JSON string
        other => other.to_string(),
    }
}

/// Render data as an ASCII table using comfy-table.
pub fn render_table(data: &Value, columns: Option<&[String]>) -> String {
    match data {
        Value::Null => "(no data)".to_string(),
        Value::Array(arr) if arr.is_empty() => "(empty)".to_string(),
        Value::Array(arr) => {
            let cols = resolve_columns(data, columns);
            if cols.is_empty() {
                // Array of scalars
                let mut table = Table::new();
                table.set_header(vec![Cell::new("value")]);
                for item in arr {
                    table.add_row(vec![Cell::new(value_to_cell(item))]);
                }
                return table.to_string();
            }
            let mut table = Table::new();
            table.set_header(cols.iter().map(Cell::new));
            for item in arr {
                let row: Vec<Cell> = cols
                    .iter()
                    .map(|col| {
                        let v = item.get(col).unwrap_or(&Value::Null);
                        Cell::new(value_to_cell(v))
                    })
                    .collect();
                table.add_row(row);
            }
            table.to_string()
        }
        Value::Object(obj) => {
            let cols = resolve_columns(data, columns);
            let mut table = Table::new();
            table.set_header(vec![Cell::new("key"), Cell::new("value")]);
            for key in &cols {
                let v = obj.get(key).unwrap_or(&Value::Null);
                table.add_row(vec![Cell::new(key), Cell::new(value_to_cell(v))]);
            }
            table.to_string()
        }
        scalar => value_to_cell(scalar),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_array_of_objects() {
        let data = json!([{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]);
        let out = render_table(&data, None);
        assert!(out.contains("Alice"));
        assert!(out.contains("Bob"));
        assert!(out.contains("name"));
        assert!(out.contains("age"));
    }

    #[test]
    fn test_single_object() {
        let data = json!({"name": "Alice", "age": 30});
        let out = render_table(&data, None);
        assert!(out.contains("Alice"));
        assert!(out.contains("key"));
        assert!(out.contains("value"));
    }

    #[test]
    fn test_empty_array() {
        let data = json!([]);
        let out = render_table(&data, None);
        assert_eq!(out, "(empty)");
    }

    #[test]
    fn test_column_selection() {
        let data = json!([{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]);
        let cols = vec!["name".to_string()];
        let out = render_table(&data, Some(&cols));
        assert!(out.contains("Alice"));
        assert!(out.contains("Bob"));
        // Should not contain age column header
        assert!(!out.contains(" age "));
    }

    #[test]
    fn test_nested_value() {
        let data = json!([{"name": "Alice", "meta": {"role": "admin"}}]);
        let out = render_table(&data, None);
        assert!(out.contains("Alice"));
        assert!(out.contains("role"));
    }
}
