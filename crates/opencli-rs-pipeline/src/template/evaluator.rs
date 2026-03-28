use serde_json::Value;
use std::collections::HashMap;

use opencli_rs_core::CliError;

use super::filters::apply_filter;
use super::parser::{BinOpKind, Expr};

/// Context available to template expressions.
pub struct TemplateContext {
    pub args: HashMap<String, Value>,
    pub data: Value,
    pub item: Value,
    pub index: usize,
}

impl Default for TemplateContext {
    fn default() -> Self {
        Self {
            args: HashMap::new(),
            data: Value::Null,
            item: Value::Null,
            index: 0,
        }
    }
}

pub fn evaluate(expr: &Expr, ctx: &TemplateContext) -> Result<Value, CliError> {
    match expr {
        Expr::IntLit(n) => Ok(Value::Number((*n).into())),
        Expr::FloatLit(f) => Ok(Value::Number(
            serde_json::Number::from_f64(*f)
                .ok_or_else(|| CliError::pipeline("Invalid float value"))?,
        )),
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::BoolLit(b) => Ok(Value::Bool(*b)),
        Expr::NullLit => Ok(Value::Null),

        Expr::Ident(name) => resolve_ident(name, ctx),

        Expr::DotAccess(base, field) => {
            let base_val = evaluate(base, ctx)?;
            Ok(access_field(&base_val, field))
        }

        Expr::BracketAccess(base, index_expr) => {
            let base_val = evaluate(base, ctx)?;
            let index_val = evaluate(index_expr, ctx)?;
            Ok(access_index(&base_val, &index_val))
        }

        Expr::FuncCall { namespace, args } => {
            let eval_args: Vec<Value> = args
                .iter()
                .map(|a| evaluate(a, ctx))
                .collect::<Result<_, _>>()?;
            call_function(namespace, &eval_args)
        }

        Expr::UnaryNot(inner) => {
            let val = evaluate(inner, ctx)?;
            Ok(Value::Bool(!is_truthy(&val)))
        }

        Expr::BinOp { left, op, right } => {
            let lval = evaluate(left, ctx)?;

            // Short-circuit for logical operators
            match op {
                BinOpKind::Or => {
                    if is_truthy(&lval) {
                        return Ok(lval);
                    }
                    return evaluate(right, ctx);
                }
                BinOpKind::And => {
                    if !is_truthy(&lval) {
                        return Ok(lval);
                    }
                    return evaluate(right, ctx);
                }
                _ => {}
            }

            let rval = evaluate(right, ctx)?;
            eval_binop(op, &lval, &rval)
        }

        Expr::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            let cond = evaluate(condition, ctx)?;
            if is_truthy(&cond) {
                evaluate(if_true, ctx)
            } else {
                evaluate(if_false, ctx)
            }
        }

        Expr::Pipe { expr, filter, args } => {
            let val = evaluate(expr, ctx)?;
            let eval_args: Vec<Value> = args
                .iter()
                .map(|a| evaluate(a, ctx))
                .collect::<Result<_, _>>()?;
            apply_filter(filter, val, &eval_args)
        }
    }
}

fn resolve_ident(name: &str, ctx: &TemplateContext) -> Result<Value, CliError> {
    match name {
        "args" => Ok(serde_json::to_value(&ctx.args).unwrap_or(Value::Null)),
        "data" => Ok(ctx.data.clone()),
        "item" => Ok(ctx.item.clone()),
        "index" => Ok(Value::Number(ctx.index.into())),
        "true" => Ok(Value::Bool(true)),
        "false" => Ok(Value::Bool(false)),
        "null" => Ok(Value::Null),
        _ => {
            // Try args as a convenience shortcut
            if let Some(val) = ctx.args.get(name) {
                Ok(val.clone())
            } else {
                Ok(Value::Null)
            }
        }
    }
}

fn access_field(val: &Value, field: &str) -> Value {
    match val {
        Value::Object(map) => map.get(field).cloned().unwrap_or(Value::Null),
        Value::Array(arr) if field == "length" => Value::Number(arr.len().into()),
        Value::String(s) if field == "length" => Value::Number(s.len().into()),
        _ => Value::Null,
    }
}

fn access_index(val: &Value, index: &Value) -> Value {
    match val {
        Value::Array(arr) => {
            if let Some(i) = index.as_u64() {
                arr.get(i as usize).cloned().unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        Value::Object(map) => {
            if let Some(key) = index.as_str() {
                map.get(key).cloned().unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}

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
        Value::Array(arr) => !arr.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn to_f64(val: &Value) -> Option<f64> {
    match val {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn eval_binop(op: &BinOpKind, left: &Value, right: &Value) -> Result<Value, CliError> {
    match op {
        BinOpKind::Add => {
            // String concat if either is string
            if let (Some(l), Some(r)) = (left.as_str(), right.as_str()) {
                return Ok(Value::String(format!("{l}{r}")));
            }
            if let (Some(l), Some(r)) = (to_f64(left), to_f64(right)) {
                let result = l + r;
                return Ok(num_to_value(result));
            }
            // String + anything
            if left.is_string() || right.is_string() {
                return Ok(Value::String(format!(
                    "{}{}",
                    value_to_display(left),
                    value_to_display(right)
                )));
            }
            Ok(Value::Null)
        }
        BinOpKind::Sub => arith(left, right, |a, b| a - b),
        BinOpKind::Mul => arith(left, right, |a, b| a * b),
        BinOpKind::Div => arith(left, right, |a, b| if b == 0.0 { f64::NAN } else { a / b }),
        BinOpKind::Mod => arith(left, right, |a, b| if b == 0.0 { f64::NAN } else { a % b }),
        BinOpKind::Gt => Ok(Value::Bool(
            cmp_values(left, right) == Some(std::cmp::Ordering::Greater),
        )),
        BinOpKind::Lt => Ok(Value::Bool(
            cmp_values(left, right) == Some(std::cmp::Ordering::Less),
        )),
        BinOpKind::Gte => Ok(Value::Bool(matches!(
            cmp_values(left, right),
            Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
        ))),
        BinOpKind::Lte => Ok(Value::Bool(matches!(
            cmp_values(left, right),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ))),
        BinOpKind::Eq => Ok(Value::Bool(left == right)),
        BinOpKind::Neq => Ok(Value::Bool(left != right)),
        BinOpKind::Or | BinOpKind::And => {
            // Already handled via short-circuit above
            unreachable!()
        }
    }
}

fn arith(left: &Value, right: &Value, f: impl Fn(f64, f64) -> f64) -> Result<Value, CliError> {
    if let (Some(l), Some(r)) = (to_f64(left), to_f64(right)) {
        Ok(num_to_value(f(l, r)))
    } else {
        Ok(Value::Null)
    }
}

fn num_to_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.is_finite() && n.abs() < (i64::MAX as f64) {
        Value::Number((n as i64).into())
    } else if let Some(num) = serde_json::Number::from_f64(n) {
        Value::Number(num)
    } else {
        Value::Null
    }
}

fn cmp_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    if let (Some(l), Some(r)) = (to_f64(left), to_f64(right)) {
        l.partial_cmp(&r)
    } else if let (Some(l), Some(r)) = (left.as_str(), right.as_str()) {
        Some(l.cmp(r))
    } else {
        None
    }
}

fn call_function(namespace: &[String], args: &[Value]) -> Result<Value, CliError> {
    let full_name: Vec<&str> = namespace.iter().map(|s| s.as_str()).collect();
    match full_name.as_slice() {
        ["Math", "min"] => {
            let result = args
                .iter()
                .filter_map(|v| v.as_f64())
                .fold(f64::INFINITY, f64::min);
            if result == f64::INFINITY {
                Ok(Value::Null)
            } else {
                Ok(num_to_value(result))
            }
        }
        ["Math", "max"] => {
            let result = args
                .iter()
                .filter_map(|v| v.as_f64())
                .fold(f64::NEG_INFINITY, f64::max);
            if result == f64::NEG_INFINITY {
                Ok(Value::Null)
            } else {
                Ok(num_to_value(result))
            }
        }
        ["Math", "abs"] => {
            let val = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(num_to_value(val.abs()))
        }
        ["Math", "floor"] => {
            let val = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(num_to_value(val.floor()))
        }
        ["Math", "ceil"] => {
            let val = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(num_to_value(val.ceil()))
        }
        ["Math", "round"] => {
            let val = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(num_to_value(val.round()))
        }
        _ => Err(CliError::pipeline(format!(
            "Unknown function: {}",
            namespace.join(".")
        ))),
    }
}

fn value_to_display(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => serde_json::to_string(val).unwrap_or_default(),
    }
}
