# Architecture

A self-hosted gateway that lets engineers query production databases **safely**:
every statement is parsed, checked against the caller's access level and the
cluster's policy, rewritten with hard limits, executed inside a guarded
transaction, masked, and audited. Engineers never see credentials.

## Goals

1. **Safe by construction** — read-only by default; writes need an explicit
   grant, a policy that allows them, and (by default) a second person's approval.
2. **No production harm** — statement/lock timeouts, row and byte caps, cost
   gate via `EXPLAIN`, per-cluster concurrency limits, read-replica routing,
   server-side cancellation.
3. **One app for every database** — PostgreSQL and MySQL (including MariaDB,
   RDS, Aurora, Cloud SQL, Azure Database, AlloyDB and self-hosted); the engine
   layer is an enum so more engines can be added.
4. **Easy to deploy and scale** — one static binary with the UI embedded;
   stateless app nodes; all state in a Postgres metadata store; health checks
   leader-elected via advisory lock so N replicas don't multiply load.
5. **Strict Rust** — `unsafe_code = "forbid"`, no `unwrap`/`expect`/`panic` in
   non-test code (workspace lints).

For the threat model and limits, see [Security model](SECURITY-MODEL.md).

## Components

```
            ┌──────────────── browser (React SPA, embedded) ───────────────┐
            │  console · schema explorer · approvals · audit · admin       │
            └──────────────────────────────┬───────────────────────────────┘
                                           │ HTTPS, session cookie
┌──────────────────────────────────────────▼───────────────────────────────────┐
│ vda-server (axum)                                                            │
│  network gate (CIDR allowlist) → auth (session) → RBAC (grants)              │
│  → query pipeline:                                                           │
│      vda-guard::analyze  → cost gate (EXPLAIN) → concurrency permit          │
│      → vda-connectors::execute_* (guarded txn) → masking → audit/history     │
│  approvals · policies · clusters/projects · users · health scheduler         │
│  secrets (AES-256-GCM) · metrics (/metrics) · /healthz /readyz               │
└───────────┬─────────────────────────────────────────────┬────────────────────┘
            │ sqlx (metadata)                             │ sqlx pools per cluster
     ┌──────▼──────┐                          ┌───────────▼──────────────────┐
     │ Postgres    │                          │ target DBs (AWS/GCP/Azure/…) │
     │ metadata    │                          │ primary + optional replica   │
     └─────────────┘                          └──────────────────────────────┘
```

### crates/vda-guard (pure, no I/O)

SQL safety engine built on `sqlparser`. Input: SQL text, dialect, caller access
level, policy. Output: per-statement classification, referenced tables and
functions, issues (with severity + stable code), risk level, overall verdict
(`allow` / `requires_approval` / `deny`), and a rewritten SQL with an enforced
`LIMIT`. Walks the **whole AST** (CTEs, subqueries, set operations) so a
`WITH x AS (DELETE …) SELECT …` is a write, not a read.

### crates/vda-connectors (I/O to target DBs)

`TargetPool` enum over sqlx Postgres/MySQL pools. Executes reads in a
`READ ONLY` transaction with `SET LOCAL statement_timeout / lock_timeout`
(MySQL: `SET SESSION TRANSACTION READ ONLY`, `max_execution_time`,
`innodb_lock_wait_timeout`), streams rows and stops at the row/byte cap, always
rolls back. Writes run in a transaction that is rolled back if affected rows
exceed the cap. Cancellation kills the query server-side
(`pg_cancel_backend` / `KILL QUERY`). Also: health probe, `EXPLAIN` cost,
schema introspection, connection test.

### crates/vda-discovery (cloud inventory)

A `Provider` trait (`test`, `scan`) returning normalized database records. The
AWS implementation is described [below](#cloud-discovery-aws-rds--aurora).

### crates/vda-server (binary `visp-db-access`)

axum HTTP API + embedded SPA. Metadata in Postgres (sqlx migrations). Owns
authn/z, pool cache, policies, approvals, audit, health and discovery
schedulers, metrics.

### web/ (React + TypeScript + Vite)

Built to `web/dist`, embedded into the server binary with `rust-embed`.

## Data model (metadata store)

- `users` (id, email, name, password_hash argon2id, org_role admin|member, disabled)
- `sessions` (token_hash sha256, user_id, expires_at, ip, user_agent)
- `projects` (id, name, description)
- `clusters` (id, project_id, name, engine, provider, region, environment,
  host, port, database, username, password_enc, tls_mode, replica_host,
  replica_port, tags jsonb)
- `cluster_policies` (cluster_id PK, policy jsonb)
- `grants` (id, user_id, scope project|cluster, scope_id, level read|write|admin,
  expires_at, created_by)
- `approvals` (id, cluster_id, requester_id, sql, reason, analysis jsonb,
  status pending|approved|rejected|executing|executed|failed|expired,
  reviewer_id, review_note,
  result jsonb, timestamps)
- `query_history` (id, cluster_id, user_id, sql, verdict,
  status running|unknown|ok|blocked|error|cancelled,
  row_count, elapsed_ms, error, created_at)
- `health_checks` (cluster_id, status, latency_ms, server_version, is_replica,
  details jsonb, checked_at) — retained 7 days
- `audit_log` (id, actor_id, action, target_type, target_id, ip, details jsonb, created_at) — append-only
- `settings` (key, value jsonb) — org network policy etc.
- `discovery_sources`, `discovery_runs`, `discovered_resources` — AWS sources,
  scan history and inventory (see below)

## Access model

- Org role `admin`: manage everything, implicit `admin` on every cluster.
- Grants on a **project** (all its clusters) or a **cluster**; effective level
  = max of unexpired grants. Levels:
  - `read` — SELECT / EXPLAIN / SHOW under policy limits.
  - `write` — may submit DML; executed only if policy `allow_writes`, and via
    approval if `require_approval_for_writes` (default true in production).
  - `admin` — manage cluster policy and grants; DDL only if policy `allow_ddl`,
    always via approval.
- Four-eyes: an approver can never approve their own request.
- Grants can expire (just-in-time access).

## Query pipeline (`POST /api/v1/clusters/:id/query`)

1. Network gate: client IP ∈ org CIDRs ∩ cluster CIDRs (if set).
2. Session → user → effective access level (403 if none).
3. `vda_guard::analyze(sql, dialect, level, policy)`.
   - `deny` → 422, recorded as `blocked` in history + audit.
   - `requires_approval` → 409 `approval_required` (UI offers "Request approval").
4. Reads: optional cost gate (`EXPLAIN`, compare to `policy.max_cost`).
5. Acquire per-cluster semaphore permit (`max_concurrent_queries`), 429 if saturated
   after a short wait.
6. Route to replica if `route_reads_to_replica` and a replica is configured & healthy.
7. Execute with limits; cancellable via `POST /api/v1/queries/:query_id/cancel`.
8. Apply column masking; record history + audit; return result.

## Scaling & deployment

- Stateless nodes behind any L7 load balancer; sessions in Postgres.
- Health scheduler runs on whichever node holds `pg_try_advisory_lock`.
- Per-node pool cache (moka, idle eviction); pool sizes are small by default
  (`max_connections=5` per cluster per node) to protect production.
- Deploy: Docker image (distroless), docker-compose for single host, Helm chart
  for Kubernetes.

## Cloud discovery (AWS RDS / Aurora)

`crates/vda-discovery` defines a `Provider` trait (`test`, `scan`) returning
normalized `DiscoveredDb` records; the AWS implementation uses `aws-config`
(default credential chain) + `aws-sdk-sts` (AssumeRole with external ID,
session name `visp-db-access`, 1 h) + `aws-sdk-rds` (`DescribeDBInstances`,
`DescribeDBClusters`, paginated, regions scanned concurrently with a small
bound). The server stores sources, scan runs and discovered resources; the
leader node (same advisory-lock pattern as health checks) runs due scans.
Scans upsert by ARN, mark unseen resources `gone`, and compute drift against
imported clusters. Import is always an explicit admin action because
database credentials are required; discovered endpoints pass the same SSRF
guard as manual clusters.

Minimal IAM policy for the gateway identity / assumed role:
`rds:DescribeDBInstances`, `rds:DescribeDBClusters` (+ `sts:AssumeRole` on the
gateway identity for cross-account roles). No write permissions are used.

Setup and operations are covered in [AWS discovery](DISCOVERY.md).

## Roadmap

RDS IAM authentication; GCP Cloud SQL and Azure discovery; OIDC/SAML SSO and
SCIM; groups; just-in-time access requests; saved queries; result export;
MongoDB and SQL Server engines; SSH/bastion tunnels; classification-driven
masking; SIEM audit export.
