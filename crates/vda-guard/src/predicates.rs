//! Conservative detection of predicates that provide no write restriction.
use crate::{tables, Dialect};
use sqlparser::ast::{BinaryOperator, Expr, UnaryOperator, Value};

pub(crate) fn tautology(expr: &Expr, dialect: Dialect) -> bool {
    truth(expr, dialect) == Some(true)
}

fn truth(expr: &Expr, dialect: Dialect) -> Option<bool> {
    match expr {
        Expr::Nested(inner) => truth(inner, dialect),
        Expr::Value(v) => match v.value {
            Value::Boolean(value) => Some(value),
            Value::Number(_, _) if dialect == Dialect::MySql => integer(expr).map(|n| n != 0),
            _ => None,
        },
        Expr::IsTrue(inner) | Expr::IsNotFalse(inner) => truth(inner, dialect),
        Expr::IsFalse(inner) | Expr::IsNotTrue(inner) => truth(inner, dialect).map(|v| !v),
        Expr::IsNull(inner) => constant_null(inner),
        Expr::IsNotNull(inner) => constant_null(inner).map(|v| !v),
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr,
        } => truth(expr, dialect).map(|v| !v),
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => match (truth(left, dialect), truth(right, dialect)) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        },
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => match (truth(left, dialect), truth(right, dialect)) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        },
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } if identical_scalar(left, right, dialect) => Some(true),
        Expr::BinaryOp { left, op, right } => integer_comparison(left, op, right),
        _ => None,
    }
}

fn identical_scalar(left: &Expr, right: &Expr, dialect: Dialect) -> bool {
    match (left, right) {
        (Expr::Nested(left), right) => identical_scalar(left, right, dialect),
        (left, Expr::Nested(right)) => identical_scalar(left, right, dialect),
        (Expr::Value(l), Expr::Value(r)) => {
            l.value == r.value
                && matches!(
                    l.value,
                    Value::Number(_, _)
                        | Value::Boolean(_)
                        | Value::SingleQuotedString(_)
                        | Value::DoubleQuotedString(_)
                )
        }
        (Expr::Identifier(l), Expr::Identifier(r)) => {
            tables::cte_key(l, dialect) == tables::cte_key(r, dialect)
        }
        (Expr::CompoundIdentifier(l), Expr::CompoundIdentifier(r)) => l
            .iter()
            .map(|i| tables::cte_key(i, dialect))
            .eq(r.iter().map(|i| tables::cte_key(i, dialect))),
        _ => false,
    }
}

fn constant_null(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Nested(inner) => constant_null(inner),
        Expr::Value(v) => match v.value {
            Value::Null => Some(true),
            Value::Boolean(_)
            | Value::Number(_, _)
            | Value::SingleQuotedString(_)
            | Value::DoubleQuotedString(_) => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn integer_comparison(left: &Expr, op: &BinaryOperator, right: &Expr) -> Option<bool> {
    let (left, right) = (integer(left)?, integer(right)?);
    match op {
        BinaryOperator::Eq => Some(left == right),
        BinaryOperator::NotEq => Some(left != right),
        BinaryOperator::Gt => Some(left > right),
        BinaryOperator::GtEq => Some(left >= right),
        BinaryOperator::Lt => Some(left < right),
        BinaryOperator::LtEq => Some(left <= right),
        _ => None,
    }
}

// Checked integer arithmetic avoids float rounding and precision-based false
// positives. Other expressions remain unknown, including volatile function calls.
fn integer(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::Nested(inner) => integer(inner),
        Expr::Value(v) => match &v.value {
            Value::Number(n, _) => n.parse().ok(),
            _ => None,
        },
        Expr::UnaryOp {
            op: UnaryOperator::Plus,
            expr,
        } => integer(expr),
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => integer(expr)?.checked_neg(),
        Expr::BinaryOp { left, op, right } => {
            let (left, right) = (integer(left)?, integer(right)?);
            match op {
                BinaryOperator::Plus => left.checked_add(right),
                BinaryOperator::Minus => left.checked_sub(right),
                BinaryOperator::Multiply => left.checked_mul(right),
                _ => None,
            }
        }
        _ => None,
    }
}
