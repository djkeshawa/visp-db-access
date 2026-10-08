//! Column masking based on column names and referenced relations.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A result column, including whether its values were masked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub type_name: String,
    pub masked: bool,
}
/// Case-insensitive glob matching with linear memory and bounded pattern size.
pub fn glob(pattern: &str, value: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let value = value.to_lowercase();
    let mut previous: Vec<bool> = std::iter::once(true)
        .chain(std::iter::repeat_n(false, value.len()))
        .collect();
    for p in pattern.bytes() {
        let mut current = Vec::with_capacity(value.len() + 1);
        current.push(p == b'*' && previous.first().copied().unwrap_or(false));
        for ((diagonal, above), v) in previous
            .iter()
            .zip(previous.iter().skip(1))
            .zip(value.bytes())
        {
            current.push(if p == b'*' {
                *above || current.last().copied().unwrap_or(false)
            } else {
                (p == b'?' || p == v) && *diagonal
            });
        }
        previous = current;
    }
    previous.last().copied().unwrap_or(false)
}
/// Table-qualified patterns apply only if that table appears in the analysis.
pub fn matches_column(pattern: &str, column: &str, tables: &[String]) -> bool {
    match pattern.rsplit_once('.') {
        None => glob(pattern, column),
        Some((table, col)) => {
            glob(col, column)
                && tables.iter().any(|name| {
                    glob(table, name) || glob(table, name.rsplit('.').next().unwrap_or(name))
                })
        }
    }
}
/// Mask every matching cell and annotate the corresponding columns.
pub fn apply(
    result: &mut vda_connectors::QueryResult,
    patterns: &[String],
    tables: &[String],
) -> Vec<Column> {
    result
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let masked = patterns
                .iter()
                .any(|p| matches_column(p, &column.name, tables));
            if masked {
                for row in &mut result.rows {
                    if let Some(value) = row.get_mut(index) {
                        *value = Value::String("••••••".into());
                    }
                }
            }
            Column {
                name: column.name.clone(),
                type_name: column.type_name.clone(),
                masked,
            }
        })
        .collect()
}

/// Whether any configured pattern could cover a relation touched by this analysis.
pub fn touches_masked(analysis: &vda_guard::Analysis, patterns: &[String]) -> bool {
    analysis
        .statements
        .iter()
        .flat_map(|s| &s.tables)
        .any(|name| {
            patterns
                .iter()
                .any(|pattern| match pattern.rsplit_once('.') {
                    None => true,
                    Some((table, _)) => {
                        glob(table, name) || glob(table, name.rsplit('.').next().unwrap_or(name))
                    }
                })
        })
}

/// Reject row serialization that destroys the column names required for masking.
pub fn harden_analysis(
    analysis: &mut vda_guard::Analysis,
    patterns: &[String],
    sql: &str,
    dialect: vda_guard::Dialect,
) {
    if !touches_masked(analysis, patterns) {
        return;
    }
    let function = analysis
        .statements
        .iter()
        .flat_map(|s| &s.functions)
        .any(|name| {
            matches!(
                name.rsplit('.').next().unwrap_or(name),
                "row_to_json"
                    | "to_json"
                    | "to_jsonb"
                    | "json_agg"
                    | "jsonb_agg"
                    | "json_build_object"
                    | "jsonb_build_object"
                    | "row"
            )
        });
    if function || whole_rows(sql, dialect) {
        let issue = vda_guard::Issue {
            severity: vda_guard::Severity::Block,
            code: "masked_data_serialization".into(),
            message: "Row serialization is prohibited when the query touches masked data".into(),
        };
        analysis.issues.push(issue.clone());
        for statement in &mut analysis.statements {
            statement.issues.push(issue.clone());
        }
        analysis.verdict = vda_guard::Verdict::Deny;
        analysis.rewritten_sql = None;
    }
}

fn whole_rows(sql: &str, dialect: vda_guard::Dialect) -> bool {
    use sqlparser::{
        ast::{Expr, Select, SelectItem, SetExpr, TableFactor, Visit, Visitor},
        parser::Parser,
    };
    use std::{collections::BTreeSet, ops::ControlFlow};
    let dialect: &dyn sqlparser::dialect::Dialect = match dialect {
        vda_guard::Dialect::Postgres => &sqlparser::dialect::PostgreSqlDialect {},
        vda_guard::Dialect::MySql => &sqlparser::dialect::MySqlDialect {},
    };
    let Ok(statements) = Parser::parse_sql(dialect, sql) else {
        return true;
    };
    #[derive(Default)]
    struct Relations(BTreeSet<String>);
    impl Visitor for Relations {
        type Break = ();
        fn pre_visit_table_factor(&mut self, table: &TableFactor) -> ControlFlow<()> {
            match table {
                TableFactor::Table { name, alias, .. } => {
                    let name = name.to_string().replace(['"', '`'], "").to_lowercase();
                    self.0.insert(name.clone());
                    self.0
                        .insert(name.rsplit('.').next().unwrap_or(&name).into());
                    if let Some(alias) = alias {
                        self.0.insert(alias.name.value.to_lowercase());
                    }
                }
                TableFactor::Derived {
                    alias: Some(alias), ..
                } => {
                    self.0.insert(alias.name.value.to_lowercase());
                }
                _ => {}
            }
            ControlFlow::Continue(())
        }
    }
    /// Flags row constructors and bare relation references inside one projection.
    /// Names that are projection aliases (e.g. `count(*) AS orders`) are columns, not rows.
    struct Rows<'a> {
        relations: &'a BTreeSet<String>,
        aliases: &'a BTreeSet<String>,
    }
    impl Visitor for Rows<'_> {
        type Break = ();
        fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
            let is_relation =
                |name: String| !self.aliases.contains(&name) && self.relations.contains(&name);
            let whole = match expr {
                Expr::Tuple(_) | Expr::Struct { .. } => true,
                Expr::Identifier(i) => is_relation(i.value.to_lowercase()),
                Expr::CompoundIdentifier(ids) => is_relation(
                    ids.iter()
                        .map(|i| i.value.to_lowercase())
                        .collect::<Vec<_>>()
                        .join("."),
                ),
                _ => false,
            };
            if whole {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }
    }
    /// Only projections return data, so only they can serialize masked rows.
    struct Projections(BTreeSet<String>);
    impl Visitor for Projections {
        type Break = ();
        fn pre_visit_query(&mut self, query: &sqlparser::ast::Query) -> ControlFlow<()> {
            for select in selects(&query.body) {
                let aliases = select
                    .projection
                    .iter()
                    .filter_map(|item| match item {
                        SelectItem::ExprWithAlias { alias, .. } => Some(alias.value.to_lowercase()),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>();
                let mut rows = Rows {
                    relations: &self.0,
                    aliases: &aliases,
                };
                for item in &select.projection {
                    item.visit(&mut rows)?;
                }
            }
            ControlFlow::Continue(())
        }
    }
    fn selects(body: &SetExpr) -> Vec<&Select> {
        match body {
            SetExpr::Select(select) => vec![select.as_ref()],
            SetExpr::SetOperation { left, right, .. } => {
                let mut all = selects(left);
                all.extend(selects(right));
                all
            }
            _ => Vec::new(),
        }
    }
    let mut relations = Relations::default();
    let _ = statements.visit(&mut relations);
    statements.visit(&mut Projections(relations.0)).is_break()
}
