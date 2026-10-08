//! Restore locking syntax unsupported by sqlparser without touching literals.
use crate::Dialect;
use sqlparser::{
    dialect::{MySqlDialect, PostgreSqlDialect},
    tokenizer::{Location, Token, Tokenizer},
};
use std::collections::HashMap;

pub(crate) fn restore(
    sql: &str,
    dialect: Dialect,
    syntax: &mut impl Iterator<Item = &'static str>,
) -> String {
    if syntax.size_hint().1 == Some(0) {
        return sql.to_owned();
    }
    let pg = PostgreSqlDialect {};
    let my = MySqlDialect {};
    let tokenizer_dialect: &dyn sqlparser::dialect::Dialect = match dialect {
        Dialect::Postgres => &pg,
        Dialect::MySql => &my,
    };
    let Ok(tokens) = Tokenizer::new(tokenizer_dialect, sql).tokenize_with_location() else {
        return sql.to_owned();
    };
    let tokens: Vec<_> = tokens
        .into_iter()
        .filter(|t| !matches!(t.token, Token::Whitespace(_)))
        .collect();
    let mut replacements = vec![];
    for pair in tokens.windows(2) {
        if let (Some(first), Some(second)) = (pair.first(), pair.get(1)) {
            if matches!(&first.token, Token::Word(w) if w.quote_style.is_none() && w.value == "FOR")
                && matches!(&second.token, Token::Word(w) if w.quote_style.is_none() && matches!(w.value.as_str(), "UPDATE" | "SHARE"))
            {
                if let Some(spelling) = syntax.next() {
                    replacements.push((first.span.start, second.span.end, spelling));
                }
            }
        }
    }
    if replacements.is_empty() {
        return sql.to_owned();
    }
    let mut offsets: HashMap<_, _> = replacements
        .iter()
        .flat_map(|(start, end, _)| [(*start, 0_usize), (*end, 0)])
        .collect();
    let mut location = Location::new(1, 1);
    for (offset, character) in sql.char_indices() {
        if let Some(value) = offsets.get_mut(&location) {
            *value = offset;
        }
        if character == '\n' {
            location.line += 1;
            location.column = 1;
        } else {
            location.column += 1;
        }
    }
    if let Some(value) = offsets.get_mut(&location) {
        *value = sql.len();
    }
    let mut result = String::with_capacity(sql.len());
    let mut previous = 0;
    for (start, end, spelling) in replacements {
        if let (Some(start), Some(end)) = (offsets.get(&start), offsets.get(&end)) {
            if let Some(prefix) = sql.get(previous..*start) {
                result.push_str(prefix);
            }
            result.push_str(spelling);
            previous = *end;
        }
    }
    if let Some(suffix) = sql.get(previous..) {
        result.push_str(suffix);
    }
    result
}
