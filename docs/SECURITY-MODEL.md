# Security model

visp-db-access sits between people and production databases. This document
describes what it guarantees, how, and where its limits are. To report a
vulnerability, see [SECURITY.md](../SECURITY.md).

## What a query goes through

1. **Network gate.** The client IP must be in the organization allowlist and,
   if set, the cluster's allowlist. Forwarded headers are honored only from
   explicitly trusted proxy networks.
2. **Session and access.** Sessions are random tokens stored as SHA-256 digests;
   state-changing requests require a CSRF header. The caller's effective level
   (`read`, `write`, `admin`) is the highest unexpired grant on the cluster or
   its project; organization admins have `admin` everywhere.
3. **Analysis.** `vda-guard` parses the whole statement and classifies every
   nested statement, function and table. The verdict is `allow`,
   `requires_approval` or `deny`. Unsupported SQL fails closed.
4. **Intent recording.** Full SQL, identity, verdict and client IP are committed
   to history before any target I/O. If that write fails, the query is refused.
5. **Limits.** An optional `EXPLAIN` cost gate, a per-cluster concurrency
   permit, and statement, lock, row, byte and affected-row limits.
6. **Guarded execution.** Reads run in a read-only transaction that is always
   rolled back. Writes roll back if they exceed the affected-row cap.
7. **Masking and audit.** Matching columns are masked; the final status is
   recorded synchronously.

## Access and approvals

- Writes require a `write` grant, a policy with `allow_writes`, and by default a
  second person's approval. DDL requires `allow_ddl` and always requires
  approval. Production defaults block writes and DDL entirely.
- A reviewer can never approve their own request. Approvals expire after
  24 hours and are executed through an atomic single-use claim; both the
  requester's and the reviewer's grants are re-checked at execution time.
- Grants can expire, which supports just-in-time access.

## Credentials

- Target database passwords are encrypted with AES-256-GCM under
  `VDA_MASTER_KEY`, authenticated against their cluster's UUID. They are
  write-only in the API and never shown in the console.
- Changing any connection endpoint field requires re-entering the password,
  including connection tests that would otherwise reuse stored credentials.
- User passwords are hashed with Argon2id. Accounts lock for a period after
  ten failed sign-ins within 15 minutes, alongside per-IP throttling.
- AWS discovery stores no AWS keys; see [AWS discovery](DISCOVERY.md).

## SQL guard

The guard walks the full AST, so writes hidden in CTEs, subqueries, set
operations or `EXPLAIN ANALYZE` are classified as writes. It blocks dangerous
functions (sleeps, file access, locks, `dblink`, …), sensitive catalogs,
row-locking reads without the right access, writes without a `WHERE` clause,
and configured blocked tables. On MySQL it also denies executable comments
(`/*! … */`, `/*M! … */`), optimizer hints, session assignments and
maintenance statements, and pins the SQL mode so statement boundaries can't
shift. Permitted `SELECT`s get an outer `LIMIT max_rows + 1` so truncation is
detectable.

The guard does not resolve search paths, inspect user-defined function bodies,
or estimate plans. Database privileges remain the primary control: give
visp-db-access the least-privileged database roles that fit each cluster.

## Masking

Masking is defense in depth, not a substitute for database grants and
restricted views. It applies to returned column names (including
table-qualified patterns when the query references the table), so aliases and
computed expressions can still bypass it. Queries touching potentially masked
tables can't serialize rows through JSON functions, row constructors or
whole-row references, and their upstream error messages are hidden in responses
and history.

## Network and targets

- Use TLS at the ingress and to targets, with `verify_full` for hostname
  verification.
- Target addresses are checked when a pool is created and at every health
  check. Metadata, link-local and zero-network addresses, and IPv6-mapped,
  compatible and NAT64 forms of prohibited IPv4 addresses, are always blocked.
  Loopback targets require `VDA_ALLOW_PRIVATE_TARGETS`.
- The database driver can't separate the connected IP from the TLS hostname, so
  a DNS change between validation and connection remains a rebinding risk.
  Enforce outbound firewall rules as well.
- `/metrics` uses the same network gate as the API and, when
  `VDA_METRICS_TOKEN` is set, requires `Authorization: Bearer <token>`.

## Audit

Security-relevant changes and their audit records share a metadata transaction.
Query intent and final status are written synchronously. Auxiliary events go
through a bounded 4,096-event queue that waits at most 250 ms per event and may
lose events in a crash; permanent failures log structured `audit_dead_letter`
records. Records left `running` after a crash become `unknown` after the
maximum statement timeout plus five minutes.

## Known limits

- Concurrency limits, cancellation and login throttling are per node. Use
  load-balancer affinity so cancellation reaches the executing node.
- Approval execution claims stay consumed after a crash; a claim stuck in
  `executing` becomes `failed` with an unknown-outcome message rather than
  replaying.
- MySQL's `max_execution_time` covers only `SELECT`; writes rely on the client
  deadline and lock timeouts. Rollback requires transactional storage (InnoDB),
  MySQL DDL commits implicitly, and a connection lost during `COMMIT` leaves the
  outcome uncertain.
