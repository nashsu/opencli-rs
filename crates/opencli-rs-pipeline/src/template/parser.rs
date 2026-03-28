use pest::Parser;
use pest_derive::Parser;

use opencli_rs_core::CliError;

#[derive(Parser)]
#[grammar = "template/grammar.pest"]
pub struct ExprParser;

/// AST nodes for template expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    BoolLit(bool),
    NullLit,
    Ident(String),
    DotAccess(Box<Expr>, String),
    BracketAccess(Box<Expr>, Box<Expr>),
    FuncCall {
        namespace: Vec<String>,
        args: Vec<Expr>,
    },
    UnaryNot(Box<Expr>),
    BinOp {
        left: Box<Expr>,
        op: BinOpKind,
        right: Box<Expr>,
    },
    Ternary {
        condition: Box<Expr>,
        if_true: Box<Expr>,
        if_false: Box<Expr>,
    },
    Pipe {
        expr: Box<Expr>,
        filter: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Gt,
    Lt,
    Gte,
    Lte,
    Eq,
    Neq,
    And,
    Or,
}

pub fn parse_expression(input: &str) -> Result<Expr, CliError> {
    let pairs = ExprParser::parse(Rule::expression, input)
        .map_err(|e| CliError::pipeline(format!("Template parse error: {e}")))?;

    let expr_pair = pairs
        .into_iter()
        .next()
        .unwrap()
        .into_inner()
        .find(|p| p.as_rule() == Rule::pipe_expr)
        .unwrap();

    parse_pipe_expr(expr_pair)
}

use pest::iterators::Pair;

/// Dispatch to the correct parse function based on the pair's rule.
fn parse_any(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    match pair.as_rule() {
        Rule::pipe_expr => parse_pipe_expr(pair),
        Rule::ternary_expr => parse_ternary_expr(pair),
        Rule::or_expr => parse_or_expr(pair),
        Rule::and_expr => parse_and_expr(pair),
        Rule::comparison_expr => parse_comparison_expr(pair),
        Rule::additive_expr => parse_additive_expr(pair),
        Rule::multiplicative_expr => parse_multiplicative_expr(pair),
        Rule::unary_expr => parse_unary_expr(pair),
        Rule::postfix_expr => parse_postfix_expr(pair),
        Rule::primary => parse_primary(pair),
        Rule::expr_atom => {
            let inner = pair.into_inner().next().unwrap();
            parse_any(inner)
        }
        Rule::expression => {
            let inner = pair
                .into_inner()
                .find(|p| p.as_rule() == Rule::pipe_expr)
                .unwrap();
            parse_pipe_expr(inner)
        }
        _ => Err(CliError::pipeline(format!(
            "Unexpected rule in dispatch: {:?}",
            pair.as_rule()
        ))),
    }
}

fn parse_pipe_expr(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let mut inner = pair.into_inner();
    let first = inner.next().unwrap();
    let mut expr = parse_any(first)?;

    for filter_pair in inner {
        // Each remaining pair is a filter
        let mut filter_inner = filter_pair.into_inner();
        let name = filter_inner.next().unwrap().as_str().to_string();
        let mut args = Vec::new();
        if let Some(filter_args) = filter_inner.next() {
            for arg in filter_args.into_inner() {
                args.push(parse_any(arg)?);
            }
        }
        expr = Expr::Pipe {
            expr: Box::new(expr),
            filter: name,
            args,
        };
    }

    Ok(expr)
}

fn parse_ternary_expr(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let mut inner = pair.into_inner();
    let cond = parse_any(inner.next().unwrap())?;

    if let Some(if_true_pair) = inner.next() {
        let if_true = parse_any(if_true_pair)?;
        let if_false = parse_any(inner.next().unwrap())?;
        Ok(Expr::Ternary {
            condition: Box::new(cond),
            if_true: Box::new(if_true),
            if_false: Box::new(if_false),
        })
    } else {
        Ok(cond)
    }
}

fn parse_or_expr(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let mut inner = pair.into_inner();
    let mut left = parse_any(inner.next().unwrap())?;
    for right_pair in inner {
        let right = parse_any(right_pair)?;
        left = Expr::BinOp {
            left: Box::new(left),
            op: BinOpKind::Or,
            right: Box::new(right),
        };
    }
    Ok(left)
}

fn parse_and_expr(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let mut inner = pair.into_inner();
    let mut left = parse_any(inner.next().unwrap())?;
    for right_pair in inner {
        let right = parse_any(right_pair)?;
        left = Expr::BinOp {
            left: Box::new(left),
            op: BinOpKind::And,
            right: Box::new(right),
        };
    }
    Ok(left)
}

fn parse_comparison_expr(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let mut inner = pair.into_inner();
    let left = parse_any(inner.next().unwrap())?;

    if let Some(op_pair) = inner.next() {
        let op = match op_pair.as_str() {
            ">=" => BinOpKind::Gte,
            "<=" => BinOpKind::Lte,
            "!=" => BinOpKind::Neq,
            "==" => BinOpKind::Eq,
            ">" => BinOpKind::Gt,
            "<" => BinOpKind::Lt,
            other => {
                return Err(CliError::pipeline(format!(
                    "Unknown comparison operator: {other}"
                )))
            }
        };
        let right = parse_any(inner.next().unwrap())?;
        Ok(Expr::BinOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        })
    } else {
        Ok(left)
    }
}

fn parse_additive_expr(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let mut inner: Vec<_> = pair.into_inner().collect();
    let mut left = parse_any(inner.remove(0))?;

    let mut i = 0;
    while i < inner.len() {
        let op = match inner[i].as_str() {
            "+" => BinOpKind::Add,
            "-" => BinOpKind::Sub,
            other => {
                return Err(CliError::pipeline(format!(
                    "Unknown additive operator: {other}"
                )))
            }
        };
        i += 1;
        let right = parse_any(inner.remove(i))?;
        left = Expr::BinOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        };
    }
    Ok(left)
}

fn parse_multiplicative_expr(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let mut inner: Vec<_> = pair.into_inner().collect();
    let mut left = parse_any(inner.remove(0))?;

    let mut i = 0;
    while i < inner.len() {
        let op = match inner[i].as_str() {
            "*" => BinOpKind::Mul,
            "/" => BinOpKind::Div,
            "%" => BinOpKind::Mod,
            other => {
                return Err(CliError::pipeline(format!(
                    "Unknown multiplicative operator: {other}"
                )))
            }
        };
        i += 1;
        let right = parse_any(inner.remove(i))?;
        left = Expr::BinOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        };
    }
    Ok(left)
}

fn parse_unary_expr(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let mut inner = pair.into_inner();
    let first = inner.next().unwrap();

    if first.as_rule() == Rule::not_op {
        let operand = parse_any(inner.next().unwrap())?;
        Ok(Expr::UnaryNot(Box::new(operand)))
    } else {
        parse_any(first)
    }
}

fn parse_postfix_expr(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let mut inner = pair.into_inner();
    let mut expr = parse_any(inner.next().unwrap())?;

    for suffix in inner {
        match suffix.as_rule() {
            Rule::dot_access => {
                let field = suffix.into_inner().next().unwrap().as_str().to_string();
                expr = Expr::DotAccess(Box::new(expr), field);
            }
            Rule::bracket_access => {
                let index_expr = parse_any(suffix.into_inner().next().unwrap())?;
                expr = Expr::BracketAccess(Box::new(expr), Box::new(index_expr));
            }
            _ => {}
        }
    }
    Ok(expr)
}

fn parse_primary(pair: Pair<'_, Rule>) -> Result<Expr, CliError> {
    let inner = pair.into_inner().next().unwrap();

    match inner.as_rule() {
        Rule::float_lit => {
            let val: f64 = inner
                .as_str()
                .parse()
                .map_err(|_| CliError::pipeline(format!("Invalid float: {}", inner.as_str())))?;
            Ok(Expr::FloatLit(val))
        }
        Rule::int_lit => {
            let val: i64 = inner
                .as_str()
                .parse()
                .map_err(|_| CliError::pipeline(format!("Invalid integer: {}", inner.as_str())))?;
            Ok(Expr::IntLit(val))
        }
        Rule::bool_lit => Ok(Expr::BoolLit(inner.as_str() == "true")),
        Rule::null_lit => Ok(Expr::NullLit),
        Rule::string_lit => {
            let s = inner.into_inner().next().unwrap().as_str().to_string();
            Ok(Expr::StringLit(s))
        }
        Rule::func_call => {
            let mut parts = inner.into_inner();
            let mut namespace = Vec::new();
            let mut args = Vec::new();

            for part in parts.by_ref() {
                match part.as_rule() {
                    Rule::ident => namespace.push(part.as_str().to_string()),
                    Rule::func_args => {
                        for arg in part.into_inner() {
                            args.push(parse_any(arg)?);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Expr::FuncCall { namespace, args })
        }
        Rule::ident => Ok(Expr::Ident(inner.as_str().to_string())),
        Rule::pipe_expr => parse_any(inner),
        _ => Err(CliError::pipeline(format!(
            "Unexpected rule in primary: {:?}",
            inner.as_rule()
        ))),
    }
}
