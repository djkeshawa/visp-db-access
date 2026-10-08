#!/usr/bin/env bash
# Unprivileged PostgreSQL 14 for local development, extracted from the Ubuntu
# archive packages via `apt-get download` (no root, no Docker, no system install).
# Binaries/data stay in ~/.local/share/vda-postgres so they survive reboots.
# Usage: scripts/local-postgres.sh start|stop|status|seed
# Local roles: vda (superuser, trust auth on loopback), shop_reader/shop_reader_pw,
# shop_writer/shop_writer_pw. Loopback only; never use these in production.
set -euo pipefail
umask 077
repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
base=${VDA_LOCAL_PG_DIR:-$HOME/.local/share/vda-postgres}
root="$base/root"
bin="$root/usr/lib/postgresql/14/bin"
data="$base/data"
port=55432
export LD_LIBRARY_PATH="$root/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
psql_() { "$bin/psql" -X -q -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U vda "$@"; }
running() { "$bin/pg_ctl" -D "$data" status >/dev/null 2>&1; }
install() {
  [[ -x $bin/postgres ]] && return
  mkdir -p "$base/debs" "$root"
  (cd "$base/debs" && apt-get download postgresql-14 postgresql-client-14 libpq5)
  for deb in "$base"/debs/*.deb; do dpkg -x "$deb" "$root"; done
  rm -rf -- "$base/debs"
}
start() {
  install
  if [[ ! -s $data/PG_VERSION ]]; then
    "$bin/initdb" -D "$data" -U vda --auth=trust --encoding=UTF8 --no-locale >"$base/init.log"
  fi
  if running; then
    printf 'Postgres already running on 127.0.0.1:%s\n' "$port"
  else
    "$bin/pg_ctl" -D "$data" -l "$base/postgres.log" -w \
      -o "-h 127.0.0.1 -p $port -k $base" start >/dev/null
    printf 'Postgres 14 running on 127.0.0.1:%s\n' "$port"
  fi
  psql_ -d postgres <<'SQL'
SELECT 'CREATE DATABASE vda_meta' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname='vda_meta')\gexec
SELECT 'CREATE DATABASE shop' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname='shop')\gexec
SELECT 'CREATE ROLE shop_reader LOGIN PASSWORD ''shop_reader_pw''' WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname='shop_reader')\gexec
SELECT 'CREATE ROLE shop_writer LOGIN PASSWORD ''shop_writer_pw''' WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname='shop_writer')\gexec
GRANT CONNECT ON DATABASE shop TO shop_reader, shop_writer;
SQL
}
case ${1:-status} in
  start) start ;;
  stop)
    if running; then "$bin/pg_ctl" -D "$data" -w stop >/dev/null; printf 'Postgres stopped\n'
    else printf 'Postgres is stopped\n'; fi ;;
  status)
    if running; then psql_ -d postgres -A -t -c "SELECT version()"; else printf 'Postgres is stopped\n'; exit 1; fi ;;
  seed)
    start
    if [[ $(psql_ -d shop -A -t -c "SELECT to_regclass('public.customers') IS NULL") == t ]]; then
      psql_ -d shop -f "$repo_dir/deploy/demo/postgres.sql"
      psql_ -d shop <<'SQL'
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO shop_reader, shop_writer;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO shop_reader;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO shop_writer;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO shop_writer;
SQL
    fi
    psql_ -d shop -A -t -c "SELECT 'customers: ' || COUNT(*) FROM customers" ;;
  *) printf 'Usage: %s start|stop|status|seed\n' "$0" >&2; exit 2 ;;
esac
