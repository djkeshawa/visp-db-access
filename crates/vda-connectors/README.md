# vda-connectors

Guarded SQL execution for PostgreSQL and MySQL using SQLx 0.8. SQL safety
analysis and authorization belong to `vda-guard` and the server.

- Lazy pools disable statement caching, use zero minimum connections, apply TLS
  policy and session defaults, and test connections before acquisition.
- Reads run in read-only transactions. Rows and serialized row-array bytes are
  bounded; an overflow row is discarded. A truncated stream is cancelled and
  its connection closed, which rolls back its transaction without draining the
  remainder of a potentially large result.
- Writes commit only within the affected-row cap. PostgreSQL RETURNING storage
  is bounded while command-completion messages preserve the full affected count.
- Cancellation uses a fresh control connection with a one-second deadline, even
  if the pool is full. Cancelled, timed-out and dropped execution futures never
  return an unfinished transaction to the pool.
- Health probes have a five-second total deadline and preserve successful
  diagnostic fields when another field is unavailable. Schema and EXPLAIN run
  through the same read-only execution machinery.

MySQL `max_execution_time` applies to SELECT, so writes also rely on the client
hard timeout and lock deadlines. Rollback guarantees require transactional
storage engines; callers must guard statements that implicitly commit (such as
MySQL DDL). A connection loss during COMMIT can leave the outcome uncertain.

SQLx exposes one pool-acquisition deadline, covering queue wait, handshake and
session initialization. The pool applies the smaller of `acquire_timeout` and
`connect_timeout` to honor both upper bounds. MySQL has no SQLx option for a
custom connection attribute; its application label is stored as the session
variable `@vda_application_name`.

SQL integer values beyond JavaScript's exact range and arbitrary precision
numeric/decimal values become strings. Binary values become capped base64
strings. Unsupported driver types become explicit `<unsupported type: NAME>`
values. PostgreSQL scalar arrays support NULL elements and preserve multidimensional
array shapes.

See [tests/README.md](tests/README.md) for commands and disposable database setup.

Local verification uses [scripts/local-mysql.sh](../../scripts/local-mysql.sh)
(MySQL 8.4 LTS on 33306) and the optional
[scripts/local-mariadb.sh](../../scripts/local-mariadb.sh) (MariaDB 11.4 on 33307).
Both run without root or Docker. See [Development](../../docs/DEVELOPMENT.md).

Each MySQL execution pins UTC and a known SQL mode with backslash escapes enabled
and ANSI_QUOTES disabled. Trusted transaction commands use the text protocol,
since MySQL cannot prepare `START TRANSACTION READ ONLY/WRITE`. MariaDB falls back
from `max_execution_time` to fractional-second `max_statement_time` and from
`transaction_read_only` to `tx_read_only`; error 1969 is a timeout. EXPLAIN supports
MySQL `query_block.cost_info.query_cost` and MariaDB 11.4 `query_block.cost`.

BIT uses its binary wire payload; GEOMETRY is an explicit unsupported-type value.
BINARY(16) UUIDs retain their bytes as base64. SQLx 0.8 discards MySQL charset
metadata and labels some text with binary collations as binary. Introspection
explicitly converts catalog strings to a text collation. MariaDB JSON is a text
alias rather than MySQL's native JSON wire type: JSON functions return strings,
while stored JSON with binary collation is shown as base64. To display such a
column as text, select `CONVERT(doc USING utf8mb4) COLLATE utf8mb4_general_ci`.
The connector never guesses whether arbitrary BLOB bytes are JSON or UUIDs.
