# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/). Before 1.0, minor releases may
change the API and storage schema; migrations run automatically at startup.

## [Unreleased]

## [0.1.0] - 2026-10-10

First public release.

### Added

- Whole-statement SQL analysis for PostgreSQL and MySQL (including MariaDB and
  managed variants), covering CTEs, subqueries and nested writes. Unsupported
  SQL fails closed.
- Read-only access by default; writes need a grant, a permitting policy and,
  by default, four-eyes approval. DDL always needs approval. Approvals expire
  after 24 hours and are claimed once at execution.
- Production protection: statement and lock timeouts, row, byte and
  affected-row caps, an optional `EXPLAIN` cost gate, per-cluster concurrency
  limits, read-replica routing and server-side cancellation.
- Advisory performance suggestions (`SELECT *`, unbounded reads,
  leading-wildcard `LIKE`, non-sargable filters, large `OFFSET`s and `IN`
  lists) with one-click fixes. Suggestions never change the verdict.
- Column masking, including DML `RETURNING`; expiring project and cluster
  grants; organization and cluster network allowlists; an append-only audit
  log with full query history.
- CSV and JSON result export with spreadsheet formula escaping.
- AWS discovery of RDS and Aurora across accounts and regions, with
  credential-verified import and drift detection.
- A single Rust binary with the React console embedded, a PostgreSQL metadata
  store, a distroless Docker image and a Helm chart.

[Unreleased]: https://github.com/djkeshawa/visp-db-access/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/djkeshawa/visp-db-access/releases/tag/v0.1.0
