//! Whole-AST facts, with query-local CTE visibility and write checks.
use crate::{
    classify, functions, predicates, rewrite, tables, Dialect, Issue, Severity, StatementKind,
};
use sqlparser::ast::{
    Expr, FromTable, JoinOperator, ObjectName, Query, Select, SelectItem, SetExpr,
    ShowCreateObject, Statement, TableFactor, TableObject, Value, Visit, Visitor,
};
use std::{
    collections::{BTreeSet, HashMap},
    ops::ControlFlow,
};

#[derive(Default)]
pub(crate) struct Facts {
    pub tables: BTreeSet<String>,
    pub functions: BTreeSet<String>,
    pub issues: Vec<Issue>,
    pub writes: Vec<StatementKind>,
    pub select_into: bool,
    pub locking: bool,
    pub has_where: bool,
    pub broad_read: bool,
    pub cross_join: bool,
}

struct Scope {
    active: BTreeSet<String>,
    children: HashMap<usize, BTreeSet<String>>,
}

struct Walker {
    facts: Facts,
    scopes: Vec<Scope>,
    table_functions: Vec<Option<String>>,
    dialect: Dialect,
    executes: bool,
}

pub(crate) fn collect(statement: &Statement, dialect: Dialect) -> Facts {
    let mut walker = Walker {
        facts: Facts::default(),
        scopes: vec![],
        table_functions: vec![],
        dialect,
        executes: !classify::is_plan_only(statement),
    };
    let _ = statement.visit(&mut walker);
    walker.facts
}

impl Walker {
    fn function(&mut self, name: &ObjectName) {
        let name = name
            .0
            .iter()
            .map(|p| {
                p.as_ident()
                    .map_or_else(|| p.to_string().to_lowercase(), |i| i.value.to_lowercase())
            })
            .collect::<Vec<_>>()
            .join(".");
        if self.facts.functions.insert(name.clone()) {
            if let Some(issue) = functions::inspect(&name, self.dialect) {
                self.facts.issues.push(issue);
            }
        }
    }

    fn relation(&mut self, name: &ObjectName) {
        let normalized = tables::normalize(name);
        if self.table_functions.last().and_then(Option::as_ref) == Some(&normalized) {
            return;
        }
        if name.0.len() == 1 {
            if let Some(ident) = name.0.first().and_then(|p| p.as_ident()) {
                let key = tables::cte_key(ident, self.dialect);
                if self.scopes.last().is_some_and(|s| s.active.contains(&key)) {
                    return;
                }
            }
        }
        self.facts.tables.insert(normalized);
    }

    fn write_target(&mut self, factor: &TableFactor) {
        if let TableFactor::Table {
            name, args: None, ..
        } = factor
        {
            // A CTE can supply rows, but cannot replace a physical DML target.
            self.facts.tables.insert(tables::normalize(name));
        }
    }

    fn write_predicate(&mut self, selection: &Option<Expr>) {
        self.facts.has_where |= selection.is_some();
        if !self.executes {
            return;
        }
        match selection {
            None => self.facts.issues.push(Issue::new(Severity::Block, "write_without_where", "UPDATE/DELETE without WHERE can affect every row. Add a restrictive WHERE clause.")),
            Some(expr) if predicates::tautology(expr, self.dialect) => self.facts.issues.push(Issue::new(Severity::Block, "tautological_where", "This WHERE clause is trivially true and does not restrict the write. Filter by specific keys or values.")),
            Some(_) => {}
        }
    }

    fn table_bodies(&mut self, body: &SetExpr) {
        let mut pending = vec![body];
        while let Some(body) = pending.pop() {
            match body {
                SetExpr::Table(t) => {
                    if let Some(name) = &t.table_name {
                        let normalized = t.schema_name.as_ref().map_or_else(
                            || name.to_lowercase(),
                            |s| format!("{}.{}", s.to_lowercase(), name.to_lowercase()),
                        );
                        self.facts.tables.insert(normalized);
                    }
                }
                SetExpr::SetOperation { left, right, .. } => {
                    pending.push(left);
                    pending.push(right);
                }
                _ => {}
            }
        }
    }
}

impl Visitor for Walker {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        let address = query as *const Query as usize;
        let inherited = self
            .scopes
            .last()
            .map(|s| s.children.get(&address).unwrap_or(&s.active).clone())
            .unwrap_or_default();
        let mut active = inherited.clone();
        let mut preceding = inherited;
        let mut children = HashMap::new();
        if let Some(with) = &query.with {
            for cte in &with.cte_tables {
                let name = tables::cte_key(&cte.alias.name, self.dialect);
                if with.recursive {
                    preceding.insert(name.clone());
                }
                children.insert(
                    cte.query.as_ref() as *const Query as usize,
                    preceding.clone(),
                );
                preceding.insert(name.clone());
                active.insert(name);
            }
        }
        self.scopes.push(Scope { active, children });
        self.facts.locking |= !query.locks.is_empty() && self.executes;
        self.table_bodies(&query.body);
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _: &Query) -> ControlFlow<Self::Break> {
        self.scopes.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        self.facts.has_where |= select.selection.is_some();
        self.facts.select_into |= select.into.is_some() && self.executes;
        if let Some(into) = &select.into {
            for target in &into.targets {
                let name = match target {
                    Expr::Identifier(ident) => Some(ObjectName::from(ident.clone())),
                    Expr::CompoundIdentifier(idents) => Some(ObjectName::from(idents.clone())),
                    _ => None,
                };
                if let Some(name) = name {
                    self.facts.tables.insert(tables::normalize(&name));
                }
            }
        }
        let table_count: usize = select.from.iter().map(|t| 1 + t.joins.len()).sum();
        self.facts.broad_read |= table_count >= 4
            || (table_count > 0
                && select.selection.is_none()
                && select.projection.iter().any(|p| {
                    matches!(
                        p,
                        SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)
                    )
                }));
        self.facts.cross_join |= select.from.len() > 1
            || select
                .from
                .iter()
                .flat_map(|t| &t.joins)
                .any(|j| matches!(j.join_operator, JoinOperator::CrossJoin(_)));
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, name: &ObjectName) -> ControlFlow<Self::Break> {
        self.relation(name);
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<Self::Break> {
        let function = match factor {
            TableFactor::Table {
                name,
                args: Some(_),
                ..
            }
            | TableFactor::Function { name, .. } => {
                self.function(name);
                Some(tables::normalize(name))
            }
            _ => None,
        };
        self.table_functions.push(function);
        ControlFlow::Continue(())
    }

    fn post_visit_table_factor(&mut self, _: &TableFactor) -> ControlFlow<Self::Break> {
        self.table_functions.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        match expr {
            Expr::Function(function) => self.function(&function.name),
            Expr::Like { pattern, .. } | Expr::ILike { pattern, .. } if matches!(pattern.as_ref(), Expr::Value(v) if matches!(&v.value, Value::SingleQuotedString(s) if s.starts_with('%'))) =>
            {
                self.facts.issues.push(Issue::new(Severity::Info, "non_sargable_like", "A leading-wildcard LIKE may scan the entire table instead of using an index. Prefer a prefix match or a search index."));
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
        for name in tables::supplemental(statement) {
            self.facts.tables.insert(tables::normalize(&name));
        }
        let kind = classify::base(statement);
        if classify::is_write(kind) && self.executes {
            self.facts.writes.push(kind);
        }
        match statement {
            Statement::Update(update) => {
                self.write_target(&update.table.relation);
                self.write_predicate(&update.selection);
            }
            Statement::Delete(delete) => {
                let (FromTable::WithFromKeyword(from) | FromTable::WithoutKeyword(from)) =
                    &delete.from;
                for table in from {
                    self.write_target(&table.relation);
                }
                for name in &delete.tables {
                    self.facts.tables.insert(tables::normalize(name));
                }
                self.write_predicate(&delete.selection);
            }
            Statement::Call(function) => self.function(&function.name),
            Statement::ShowCreate {
                obj_type: ShowCreateObject::Table | ShowCreateObject::View,
                obj_name,
            } => {
                self.facts.tables.insert(tables::normalize(obj_name));
            }
            Statement::Merge(merge) => self.write_target(&merge.table),
            Statement::Insert(insert) => {
                match &insert.table {
                    TableObject::TableName(name) => {
                        self.facts.tables.insert(tables::normalize(name));
                    }
                    TableObject::TableFunction(function) => self.function(&function.name),
                    TableObject::TableQuery(_) => {}
                }
                if !self.executes {
                    return ControlFlow::Continue(());
                }
                if let Some(source) = &insert.source {
                    if !matches!(source.body.as_ref(), SetExpr::Values(_))
                        && !rewrite::has_limit(source)
                    {
                        self.facts.issues.push(Issue::new(Severity::Warning, "unbounded_insert_select", "INSERT ... SELECT has no outer LIMIT and may insert many rows. Add a LIMIT or constrain the source."));
                    }
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

pub(crate) fn aggregate_read(body: &SetExpr) -> bool {
    match body {
        SetExpr::Select(select) => !select.projection.is_empty() && select.projection.iter().all(|item| {
            let expr = match item { SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => expr, _ => return false };
            matches!(expr, Expr::Function(f) if f.over.is_none() && functions::is_aggregate(&tables::normalize(&f.name)))
        }),
        SetExpr::Query(query) => aggregate_read(&query.body),
        SetExpr::SetOperation { left, right, .. } => aggregate_read(left) && aggregate_read(right),
        _ => false,
    }
}
