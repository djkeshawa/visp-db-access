# Connector tests

Unit tests and lazy-pool tests need no database:

```sh
cargo test -p vda-connectors
```

Live tests skip with a note when their connection URL is unset. Use **disposable**
databases: tests create uniquely named tables and drop them on success. A failed
test can leave its table behind. Each test uses a single pool slot, exercising
cancellation without spare pooled connections.

For PostgreSQL 17:

```sh
docker run --rm --name vda-test-pg -e POSTGRES_PASSWORD=pw -p 55432:5432 postgres:17
VDA_TEST_PG_URL='postgres://postgres:pw@127.0.0.1:55432/postgres?sslmode=disable' \
  cargo test -p vda-connectors --test guarded postgres -- --nocapture
```

For MySQL 8.4:

```sh
docker run --rm --name vda-test-mysql -e MYSQL_ROOT_PASSWORD=pw -e MYSQL_DATABASE=vda_test -p 53306:3306 mysql:8.4
VDA_TEST_MYSQL_URL='mysql://root:pw@127.0.0.1:53306/vda_test?ssl-mode=disabled' \
  cargo test -p vda-connectors --test guarded mysql -- --nocapture
```

Wait for the database's startup log to say it is ready before testing. Set both
variables to run both engines together. The test role needs create/drop-table
permissions and permission to cancel its own sessions. MySQL tests use InnoDB
(the default); affected-row rollback cannot undo changes to nontransactional
storage engines. `SELECT SLEEP` can be interrupted by MySQL without an error;
the client deadline also enforces the timeout.

Coverage includes row/byte truncation, empty-result metadata, read-only
transactions, cancellation and dropped futures, timeouts, rollback on affected
row caps, bounded PostgreSQL RETURNING, schema, health, EXPLAIN and type decoding.

Without Docker, use the local database scripts:

```sh
scripts/local-postgres.sh seed
scripts/local-mysql.sh seed
scripts/local-mariadb.sh seed # optional MariaDB 11.4 compatibility
VDA_TEST_PG_URL=postgres://vda@127.0.0.1:55432/shop \
VDA_TEST_MYSQL_URL=mysql://vda:vda_local_pw@127.0.0.1:33306/shop \
VDA_TEST_MYSQL_PASSWORD=vda_local_pw \
cargo test -p vda-connectors -- --nocapture
```

Set port 33307 in the MySQL URL to run the same tests against MariaDB. The
additional `VDA_TEST_MYSQL_PASSWORD` enables the lazy factory/session-default
regression without assuming the URL's credentials. Both URL and password must
refer to a disposable admin account. Live coverage includes stored TINYINT(1),
BIT(9), DECIMAL(44,4), DATETIME(6), TIMESTAMP(6), negative and >24-hour TIME, YEAR,
JSON (engine-specific wire representation), ENUM, SET, BLOB, BINARY(16), GEOMETRY,
unsigned BIGINT and utf8mb4. Guard-approved comments and escaped literals execute
through the connector. A CPU-intensive cross join proves the server execution
timer fires before the client's fallback deadline; single-slot cancellation
asserts that the following query uses a different connection.
