# HTTP API contract (v1)

This file is the contract between `crates/vda-server` and `web/`. Change it
only deliberately and update both sides.

Conventions

- Base path `/api/v1`. JSON bodies, `snake_case` fields. IDs are UUID strings.
  Timestamps are RFC 3339 strings (UTC).
- Auth: HttpOnly cookie `vda_session` set by login. Mutating requests must send
  header `X-Requested-With: vda` (CSRF guard); server rejects otherwise (403).
- Errors: non-2xx with `{"error": {"code": "string", "message": "string", "details": any|null}}`.
  Common codes: `unauthenticated` 401, `forbidden` 403, `not_found` 404,
  `validation` 400, `conflict` 409, `approval_required` 409, `query_denied` 422,
  `cost_exceeded` 422, `rate_limited` 429, `busy` 429, `upstream` 502, `internal` 500.
- Paginated lists (grants, approvals, history, audit): `{"items": [...], "next_cursor": "string|null"}`;
  query `?limit=50&cursor=...`. Cursors are opaque and URL-safe, ordered by creation time
  and ID descending. Other lists return `{items: [...]}`.
- JSON strings and object keys reject NUL and other C0 controls with 400 `validation`
  (`Text contains prohibited control characters`); tab, newline and carriage return are allowed.
- 429 `busy` means the cluster's query slots are occupied; 429 `rate_limited` means
  login/password work is throttled. Wait before retrying; mutations are never automatically retried.

## Shared types

```ts
type OrgRole = 'admin' | 'member';
type AccessLevel = 'read' | 'write' | 'admin';
type Engine = 'postgres' | 'mysql';
type Provider = 'aws' | 'gcp' | 'azure' | 'onprem' | 'other';
type Environment = 'production' | 'staging' | 'development';
type TlsMode = 'disable' | 'prefer' | 'require' | 'verify_full';
type HealthStatus = 'healthy' | 'degraded' | 'down' | 'unknown';
type Verdict = 'allow' | 'requires_approval' | 'deny';
type Severity = 'info' | 'warning' | 'block';
type Risk = 'low' | 'medium' | 'high' | 'critical';
type StatementKind =
  | 'select'
  | 'explain'
  | 'show'
  | 'insert'
  | 'update'
  | 'delete'
  | 'merge'
  | 'ddl'
  | 'dcl'
  | 'transaction'
  | 'utility'
  | 'other';

interface User {
  id: string;
  email: string;
  name: string;
  org_role: OrgRole;
  disabled: boolean;
  created_at: string;
  last_login_at: string | null;
}

interface Project {
  id: string;
  name: string;
  description: string;
  cluster_count: number;
  created_at: string;
}

interface Health {
  status: HealthStatus;
  latency_ms: number | null;
  server_version: string | null;
  is_replica: boolean | null;
  active_connections: number | null;
  max_connections: number | null;
  error: string | null;
  checked_at: string | null;
}

interface Cluster {
  id: string;
  project_id: string;
  name: string;
  engine: Engine;
  provider: Provider;
  region: string;
  environment: Environment;
  host: string;
  port: number;
  database: string;
  username: string;
  tls_mode: TlsMode;
  replica_host: string | null;
  replica_port: number | null;
  tags: Record<string, string>;
  health: Health;
  my_access: AccessLevel | null; // effective level of the caller
  created_at: string;
  updated_at: string;
}
// password is write-only: accepted on create/update, never returned.

interface Policy {
  max_rows: number; // default 1000 (prod) / 5000 (non-prod)
  statement_timeout_ms: number; // default 15000 (prod) / 60000
  lock_timeout_ms: number; // default 2000
  max_cost: number | null; // EXPLAIN total cost gate for reads
  max_concurrent_queries: number; // per node, default 4
  allow_writes: boolean; // default false in production
  require_approval_for_writes: boolean; // default true
  max_affected_rows: number; // default 1000
  allow_ddl: boolean; // default false
  route_reads_to_replica: boolean; // default true
  masked_columns: string[]; // patterns: "email", "users.ssn", "*.password*"
  blocked_tables: string[]; // patterns: "secrets.*", "audit_trail"
  allowed_cidrs: string[]; // empty = only org-level rule applies
}

interface Issue {
  severity: Severity;
  code: string;
  message: string;
}
interface StatementAnalysis {
  kind: StatementKind;
  sql: string;
  tables: string[];
  functions: string[];
  risk: Risk;
  issues: Issue[];
  has_where: boolean;
  has_limit: boolean;
}
interface Analysis {
  verdict: Verdict;
  risk: Risk;
  statements: StatementAnalysis[];
  rewritten_sql: string | null;
  issues: Issue[];
} // issues = flattened, incl. top-level

interface Column {
  name: string;
  type_name: string;
  masked: boolean;
}
interface QueryResult {
  query_id: string;
  columns: Column[];
  rows: unknown[][];
  row_count: number;
  truncated: boolean;
  affected_rows: number | null;
  elapsed_ms: number;
  executed_sql: string;
  routed_to: 'primary' | 'replica';
  analysis: Analysis;
}

interface HistoryEntry {
  id: string;
  cluster_id: string;
  cluster_name: string;
  user_id: string;
  user_email: string;
  sql: string;
  verdict: Verdict;
  status: 'running' | 'unknown' | 'ok' | 'blocked' | 'error' | 'cancelled';
  row_count: number | null;
  elapsed_ms: number | null;
  error: string | null;
  created_at: string;
}

interface Approval {
  id: string;
  cluster_id: string;
  cluster_name: string;
  requester: { id: string; email: string; name: string };
  sql: string;
  reason: string;
  analysis: Analysis;
  status:
    | 'pending'
    | 'approved'
    | 'rejected'
    | 'executing'
    | 'executed'
    | 'failed'
    | 'expired';
  error: string | null;
  sql_truncated: boolean;
  reviewer: { id: string; email: string; name: string } | null;
  review_note: string | null;
  result: QueryResult | null;
  created_at: string;
  reviewed_at: string | null;
  executed_at: string | null;
  expires_at: string;
}

interface Grant {
  id: string;
  user: { id: string; email: string; name: string };
  scope: 'project' | 'cluster';
  scope_id: string;
  scope_name: string;
  level: AccessLevel;
  expires_at: string | null;
  created_at: string;
  created_by: string | null;
}

interface AuditEvent {
  id: string;
  actor: { id: string; email: string } | null;
  action: string;
  target_type: string | null;
  target_id: string | null;
  ip: string | null;
  details: unknown;
  created_at: string;
}

interface SchemaTree {
  schemas: {
    name: string;
    tables: {
      name: string;
      kind: 'table' | 'view' | 'materialized_view';
      row_estimate: number | null;
      columns: {
        name: string;
        data_type: string;
        nullable: boolean;
        is_primary_key: boolean;
      }[];
    }[];
  }[];
}
```

## Endpoints

### System (no session auth, no `/api/v1` prefix)

- `GET /healthz` → `200 "ok"` — process alive.
- `GET /readyz` → `200` when metadata DB reachable, else `503`.
- `GET /metrics` → Prometheus text with `Authorization: Bearer <VDA_METRICS_TOKEN>`;
  403 if the token is absent, incorrect, or metrics access is not configured.

### Auth

- `POST /auth/login` `{email, password}` → `{user: User}` + cookie. 401 on bad creds (generic message). Rate-limited per IP and per IP+email.
  Ten failed attempts temporarily lock the email for 15 minutes across all source IPs.
  A locked or disabled account returns the same generic 401 as incorrect credentials,
  including for nonexistent accounts; clients must not infer account existence.
- `POST /auth/logout` → `204`.
- `GET /auth/me` → `{user: User}`.
- `POST /auth/change-password` `{current_password, new_password}` → `204`.

### Overview

- `GET /overview` → `{clusters_total, healthy, degraded, down, unknown,
queries_24h, blocked_24h, errors_24h, pending_approvals,
recent_queries: HistoryEntry[] (≤10), unhealthy_clusters: Cluster[]}`
  (scoped to clusters the caller can access).

### Users

- `GET /users` → `{items: User[]}` (org admin)
- `GET /users/lookup?q=` → `{items: {id, email, name}[]}` (org admin, or an unexpired
  admin grant on at least one existing project/cluster). Case-insensitive literal substring
  search in name/email; trimmed query must contain 2–254 Unicode characters (400 `validation`
  otherwise). Disabled users are excluded. At most 20 results, ordered by name, email, ID.
  Password hashes, organization roles, and account state are never included.
- `POST /users` `{email, name, password, org_role}` → `User` (org admin)
- `PATCH /users/:id` `{name?, org_role?, disabled?, password?}` → `User` (org admin)
- `DELETE /users/:id` → `204` (org admin; soft: disables + revokes sessions)

### Projects

- `GET /projects` → `{items: Project[]}` (projects with ≥1 accessible cluster, or all for admin)
- `POST /projects` `{name, description}` → `Project` (admin)
- `PATCH /projects/:id` `{name?, description?}` → `Project` (admin)
- `DELETE /projects/:id` → `204` (admin; 409 if it still has clusters)

### Clusters

- `GET /clusters?project_id=&environment=&engine=&q=` → `{items: Cluster[]}` (only accessible)
- `POST /clusters` `{project_id, name, engine, provider, region, environment, host, port,
database, username, password, tls_mode, replica_host?, replica_port?, tags?}` → `Cluster` (admin).
  Creates default policy for the environment.
- `GET /clusters/:id` → `Cluster`
- `PATCH /clusters/:id` (same fields, all optional; `password` optional) → `Cluster` (admin / cluster admin).
  Changing engine, host, port, database, username, TLS mode, replica host or replica port
  requires explicitly resupplying `password`. Otherwise 400 `validation`:
  `Changing the connection endpoint requires re-entering the password`.
  Moving a cluster to a different project requires organization administration.
- `DELETE /clusters/:id` → `204` (org admin)
- `POST /clusters/test-connection` `{engine, host, port, database, username, password?, tls_mode, cluster_id?}`
  → `Health` (if `password` omitted and `cluster_id` given, use stored password only when all endpoint fields match the stored connection; otherwise
  the same password re-entry validation applies). Org admin only.
- `GET /clusters/:id/health?hours=24` → `{current: Health, history: {status, latency_ms, checked_at}[]}`
- `POST /clusters/:id/health/check` → `Health` (run now; cluster admin)
- `GET /clusters/:id/schema` → `SchemaTree` (read; cached 5 min server-side, `?refresh=true` bypasses)

### Policy & access

- `GET /clusters/:id/policy` → `Policy` (read)
- `PUT /clusters/:id/policy` `Policy` → `Policy` (cluster admin)
- `GET /clusters/:id/grants` → `{items: Grant[]}` (includes project-level grants that apply) (cluster admin)
- `GET /grants?scope=&scope_id=&user_id=&limit=50&cursor=` → `{items: Grant[], next_cursor}`.
  Org admins see all grants, including projects with no clusters and expired grants.
  Project admins see grants on their project and its clusters. Cluster admins see grants
  on their cluster, not grants on the parent project or unrelated scopes. Administrative
  authority must be unexpired; users with no administered scope receive 403. Optional
  filters are exact matches; `scope` is `project` or `cluster`. `limit` must be 1–200.
  Invalid scope/UUID/cursor/limit returns 400 `validation`.
- `POST /grants` `{user_id, scope, scope_id, level, expires_at?}` → `Grant` (org admin, or admin on scope)
- `DELETE /grants/:id` → `204` (org admin, or admin on the grant's scope).
  Scoped admins cannot grant to themselves or delegate access beyond the expiry of
  their own administrative authority.

### Queries

- `POST /clusters/:id/analyze` `{sql}` → `Analysis` (read). Cheap; UI calls it debounced while typing.
  `masked_data_serialization` is a blocking issue for whole-row/JSON serialization
  touching masked data; select individual columns so masking can be applied.
  MySQL executable comments (`/*!...*/`, `/*M!...*/`) are always denied with
  `executable_comment`; optimizer hints are blocked with `optimizer_hint` because
  hints can override execution limits. Session assignments and maintenance commands
  are blocked. The connector pins SQL mode to `STRICT_TRANS_TABLES,NO_ENGINE_SUBSTITUTION`
  (backslash escapes enabled, ANSI_QUOTES disabled) and UTC for each execution.
  SELECT rewriting re-escapes literal backslashes; mode-dependent statement boundaries
  are rejected. Rewritten outer SELECT/FETCH limits use `max_rows + 1` when imposing/capping a
  policy limit to detect truncation. Smaller explicit limits are preserved. Display
  `rewritten_sql` verbatim; only `max_rows` rows are returned.
- `POST /clusters/:id/query` `{sql, query_id?}` → `QueryResult`.
  `query_id` (client-generated UUID) lets the client cancel before the response arrives.
  Errors: `query_denied` 422 (`details: Analysis`), `approval_required` 409 (`details: Analysis`),
  `cost_exceeded` 422 (`details: {cost, max_cost}`), `busy` 429, `upstream` 502 (`message` = DB error).
- `POST /queries/:query_id/cancel` → `204` (own queries; admins any).
- `GET /history?cluster_id=&user_id=&status=&limit=&cursor=` → `{items: HistoryEntry[], next_cursor}`.
  Members see only their own; org admins see all. `limit` must be 1–200.
  `running` is durably recorded before target execution. Abandoned executions become
  `unknown` after recovery; this does not imply success or rollback. History `id` is a
  fresh identity for each attempt and is distinct from the cancellation `query_id`.

### Approvals

- `POST /approvals` `{cluster_id, sql, reason}` → `Approval` (write level; re-analyzed server-side; 422 if `deny`)
- `GET /approvals?status=&cluster_id=&mine=true&limit=50&cursor=` → `{items: Approval[], next_cursor: string | null}`.
  Org admins see all, scoped admins see requests on administered clusters, and other
  members see their own requests. Detail/review/execution also check current access.
  Keyset pagination orders by creation time and ID descending; `limit` must be 1–100.
  List items set `result` to null, truncate `sql` to 2,000 characters, and set
  `sql_truncated` to true when truncated. `GET /approvals/:id` returns full SQL
  and result, with `sql_truncated: false`. `executing` claims are never expired;
  abandoned claims become `failed` with an `error` after the statement timeout
  plus five minutes. Failed executions remain consumed and cannot be replayed.
- `GET /approvals/:id` → `Approval`
- `POST /approvals/:id/approve` `{note?}` → `Approval` (cluster admin, ≠ requester)
- `POST /approvals/:id/reject` `{note}` → `Approval`
- `POST /approvals/:id/execute` → `Approval` with `result` (requester or approver; once; approval valid 24h)

### Audit

- `GET /audit?action=&actor_id=&limit=&cursor=` → `{items: AuditEvent[], next_cursor}` (org admin).
  `action` is an exact match; `limit` must be 1–200. `query.execute` durably records
  every authorized query attempt before target I/O, including blocked attempts;
  details contain `{query_id, history_id, status}` with initial `running`/`blocked`
  status. Final outcome is in history. Approval actions include `approval.create`,
  `approval.approve`, `approval.reject`, `approval.execute` (claim), `approval.executed`,
  and `approval.failed`. Administrative actions use `*.create`, `*.update`, `*.delete`.

### Settings

- `GET /settings/network` → `{allowed_cidrs: string[], trust_proxy_headers: boolean}` (admin)
- `PUT /settings/network` same → same. Server refuses a change that would lock out the caller's current IP (400 `validation`).

### Cloud discovery (org admin only)

Discovery finds databases in cloud accounts and proposes them for import. It
never creates clusters on its own (import needs DB credentials) and only uses
read-only cloud APIs. AWS credentials are never stored: the gateway uses its
own ambient identity (IRSA / ECS task role / instance profile / env) and,
per source, optionally assumes a role in the target account.

```ts
type DiscoveryProvider = 'aws';
type ResourceStatus = 'new' | 'imported' | 'ignored' | 'gone';
type Drift =
  'endpoint_changed' | 'replica_changed' | 'deleted' | 'engine_changed';

interface DiscoverySource {
  id: string;
  provider: DiscoveryProvider;
  name: string;
  role_arn: string | null; // arn:aws:iam::<12 digits>:role/<path/name>
  external_id_set: boolean; // external_id is write-only
  regions: string[]; // e.g. ["us-east-1","eu-west-1"], 1..=20
  default_project_id: string | null;
  environment_tag_keys: string[]; // default ["environment","env","stage"]
  scan_interval_minutes: number; // 5..=1440, default 60
  enabled: boolean;
  last_scan: ScanRun | null;
  last_test: {
    ok: boolean;
    account_id: string | null;
    identity_arn: string | null;
    tested_at: string;
  } | null;
  created_at: string;
  updated_at: string;
}

interface ScanRun {
  id: string;
  source_id: string;
  status: 'running' | 'succeeded' | 'partial' | 'failed';
  started_at: string;
  finished_at: string | null;
  found: number;
  new: number;
  gone: number;
  changed: number;
  errors: { region: string | null; message: string }[];
}

interface DiscoveredResource {
  id: string;
  source_id: string;
  source_name: string;
  provider: DiscoveryProvider;
  kind: 'rds_instance' | 'aurora_cluster';
  arn: string;
  identifier: string;
  account_id: string;
  region: string;
  engine: Engine; // postgres | mysql (aurora-*/mariadb mapped)
  engine_detail: string; // raw, e.g. "aurora-postgresql"
  engine_version: string;
  host: string;
  port: number;
  replica_host: string | null; // Aurora reader endpoint or first read replica
  replica_port: number | null;
  database: string | null; // DBName if set
  status_detail: string; // AWS status, e.g. "available"
  publicly_accessible: boolean;
  encrypted: boolean;
  multi_az: boolean;
  iam_auth_enabled: boolean;
  vpc_id: string | null;
  tags: Record<string, string>;
  suggested_environment: Environment; // from environment_tag_keys, else name heuristics, else "production"
  status: ResourceStatus;
  cluster_id: string | null; // set when imported
  drift: Drift[]; // vs the imported cluster's current config
  first_seen_at: string;
  last_seen_at: string;
}
```

- `GET /discovery/sources` → `{items: DiscoverySource[]}`
- `POST /discovery/sources` `{provider, name, role_arn?, external_id?, regions, default_project_id?, environment_tag_keys?, scan_interval_minutes?, enabled?}` → `DiscoverySource`
- `PATCH /discovery/sources/:id` (same fields, optional; `external_id: null` clears) → `DiscoverySource`
- `DELETE /discovery/sources/:id` → `204` (its resources are deleted; imported clusters are kept)
- `POST /discovery/sources/test` (same body as create) → the connection test report below.
  Tests an unsaved draft without creating a source or storing its external ID.
  A server-generated summary is retained for 10 minutes in a bounded, actor-scoped
  cache and persisted in `last_test` when that actor saves the matching role,
  external ID and regions through create/update. Tests are optional before saving.
- `POST /discovery/sources/:id/test` → `{ok: boolean, account_id: string | null, identity_arn: string | null, regions: {region: string, ok: boolean, error: string | null}[]}`
  (STS GetCallerIdentity after assume-role + a 1-item DescribeDBInstances per region).
  Both success and failure summaries persist in `last_test`; timeouts return an
  `ok: false` report. Editing role/external ID/regions clears stale verification
  unless a matching draft test exists. A test completing after an edit does not
  overwrite the edited source's summary. Summaries contain no credentials.
- `POST /discovery/sources/:id/scan` → `ScanRun` (runs now, synchronously, ≤ 60 s; 409 `conflict` if a scan for this source is already running)
- `GET /discovery/sources/:id/runs?limit=&cursor=` → `{items: ScanRun[], next_cursor}`
- `GET /discovery/resources?source_id=&status=&engine=&region=&q=&limit=&cursor=` → `{items: DiscoveredResource[], next_cursor}`;
  response also includes `counts: {new, imported, ignored, gone}` for the filter minus `status`.
- `POST /discovery/resources/:id/import` `{project_id, name, environment, database, username, password, tls_mode}`
  → `Cluster`. Host/port/engine/provider(`aws`)/region/replica/tags come from the resource;
  same validation as `POST /clusters` (SSRF guard, credential encryption, default policy).
  409 if already imported. Default `tls_mode` in UI: `verify_full`.
- `POST /discovery/resources/:id/ignore` → `DiscoveredResource`; `POST /discovery/resources/:id/unignore` → `DiscoveredResource`
- `POST /discovery/resources/:id/sync` → `Cluster` (imported only): applies discovered host/port/replica to the
  cluster. Requires `password` in body if the endpoint changed (same rule as `PATCH /clusters/:id`).

Overview adds `discovery: {new: number, gone: number, drifted: number} | null` (null for non-admins).
