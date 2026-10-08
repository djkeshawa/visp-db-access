#!/usr/bin/env bash
# Unprivileged MySQL 8.4.7 LTS, obtained from Oracle's generic minimal archive:
# https://cdn.mysql.com/archives/mysql-8.4/mysql-8.4.7-linux-glibc2.28-x86_64-minimal.tar.xz
# No Docker/system packages. Requires Linux x86_64, glibc >=2.28, libaio.so.1,
# libnuma.so.1. Downloads/data stay in ~/.local/share/vda-mysql; archives removed.
# Usage: scripts/local-mysql.sh start|stop|status|seed
# Local credentials: vda/vda_local_pw, shop_reader/shop_reader_pw,
# shop_writer/shop_writer_pw. Loopback only; never use these in production.
set -euo pipefail
umask 077
repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
base=${VDA_LOCAL_MYSQL_DIR:-$HOME/.local/share/vda-mysql}
version=8.4.7
install="$base/mysql-$version-linux-glibc2.28-x86_64-minimal"
socket="$base/mysql.sock"
pid_file="$base/mysql.pid"
client() { "$install/bin/mysql" --no-defaults --socket="$socket" -uroot "$@"; }
running() { [[ -s $pid_file ]] && client -e 'SELECT 1' >/dev/null 2>&1; }
start() {
  mkdir -p "$base"
  if running; then printf 'MySQL already running on 127.0.0.1:33306\n'; return; fi
  if [[ ! -x $install/bin/mysqld ]]; then
    curl --fail --location --retry 2 --output "$base/mysql.tar.xz" \
      "https://cdn.mysql.com/archives/mysql-8.4/mysql-$version-linux-glibc2.28-x86_64-minimal.tar.xz"
    tar -xJf "$base/mysql.tar.xz" -C "$base"
    rm -- "$base/mysql.tar.xz"
  fi
  if [[ ! -d $base/data/mysql ]]; then
    "$install/bin/mysqld" --no-defaults --basedir="$install" --datadir="$base/data" \
      --initialize-insecure --log-error="$base/init.log"
  fi
  local -a launch=(nohup)
  if command -v systemd-run >/dev/null && systemctl --user is-system-running >/dev/null 2>&1; then
    launch=(systemd-run --user --unit=vda-mysql --collect)
  fi
  "${launch[@]}" "$install/bin/mysqld" --no-defaults --basedir="$install" --datadir="$base/data" \
    --bind-address=127.0.0.1 --port=33306 --socket="$socket" --pid-file="$pid_file" \
    --mysqlx=OFF --skip-log-bin --innodb-buffer-pool-size=64M \
    --innodb-redo-log-capacity=32M --log-error="$base/mysql.log" \
    >"$base/stdout.log" 2>&1 </dev/null &
  for ((attempt=0; attempt<100; attempt++)); do
    if running; then
      client <<'SQL'
CREATE USER IF NOT EXISTS 'vda'@'127.0.0.1' IDENTIFIED BY 'vda_local_pw';
GRANT ALL PRIVILEGES ON *.* TO 'vda'@'127.0.0.1' WITH GRANT OPTION;
CREATE DATABASE IF NOT EXISTS shop CHARACTER SET utf8mb4;
CREATE USER IF NOT EXISTS 'shop_reader'@'127.0.0.1' IDENTIFIED BY 'shop_reader_pw';
GRANT SELECT ON shop.* TO 'shop_reader'@'127.0.0.1';
CREATE USER IF NOT EXISTS 'shop_writer'@'127.0.0.1' IDENTIFIED BY 'shop_writer_pw';
GRANT SELECT, INSERT, UPDATE, DELETE ON shop.* TO 'shop_writer'@'127.0.0.1';
SQL
      printf 'MySQL %s running on 127.0.0.1:33306 (PID %s)\n' "$version" "$(cat "$pid_file")"
      return
    fi
    sleep 0.2
  done
  cat "$base/mysql.log" >&2
  exit 1
}
case ${1:-status} in
  start) start ;;
  stop)
    if running; then client -e SHUTDOWN; printf 'MySQL stopped\n'; else printf 'MySQL is stopped\n'; fi ;;
  status)
    if running; then client -e 'SELECT VERSION() AS version, @@port AS port'; else printf 'MySQL is stopped\n'; exit 1; fi ;;
  seed)
    start
    if [[ $(client -N -e "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema='shop' AND table_name='customers'") == 0 ]]; then
      client < "$repo_dir/deploy/demo/mysql.sql"
    fi
    client -e 'SELECT COUNT(*) AS customers FROM shop.customers' ;;
  *) printf 'Usage: %s start|stop|status|seed\n' "$0" >&2; exit 2 ;;
esac
