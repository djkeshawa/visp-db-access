# vda-guard

Pure SQL safety analysis for PostgreSQL and MySQL, built on sqlparser.
`analyze(sql, dialect, level, policy) -> Analysis` classifies every statement
and returns a verdict (`allow`, `requires_approval` or `deny`), issues with
stable codes, and SQL rewritten with an enforced row limit.

The guard walks statements, queries, expressions, relations, and table functions
through the AST visitor. It checks nested writes, execution through EXPLAIN
ANALYZE, write predicates, row locks, function deny lists, sensitive catalogs,
and blocked-table patterns before applying access and approval policy.
CTE visibility is scoped to the query and definition order; physical DML targets
are checked even when a CTE has the same name.

Permitted top-level SELECT queries receive an outer row cap of `max_rows + 1`
when their original limit is absent, excessive, or cannot be statically bounded.
Offsets are retained. FETCH PERCENT and WITH TIES are converted to a fixed row
count. PostgreSQL key-lock modes and MySQL LOCK IN SHARE MODE keep their original
semantics in the rewritten SQL. Per-statement `has_limit` describes the original
outer query rather than the rewritten limit.

Run from the workspace root:

```sh
cargo fmt -p vda-guard --check
cargo clippy --all-targets -p vda-guard -- -D warnings
cargo test -p vda-guard
cargo test --release -p vda-guard -- --ignored
```

Tests use no database, network, Docker, or additional dependencies. They include
both dialects, every blocked function, scoped CTE and nested-expression bypasses,
serde round trips, random strings, and an ignored 10 KB / 20 ms release timing
check.

Limits: unsupported SQL fails closed with `parse_error`; MySQL file destinations
also receive `file_write`. Excessive expression depth/length and set-operation
chains are rejected before constructing a deeply recursive AST. The engine does
not resolve database search paths, inspect user-defined function bodies, or
estimate execution plans. Database privileges, transaction guards, timeouts,
and executor row/affected-row limits remain necessary in the other components.
