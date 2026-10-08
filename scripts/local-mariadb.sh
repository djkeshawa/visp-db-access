#!/usr/bin/env bash
# Optional companion to local-mysql.sh: MariaDB 11.4.5 LTS, current user only.
# Official archive: https://archive.mariadb.org/mariadb-11.4.5/bintar-linux-systemd-x86_64/mariadb-11.4.5-linux-systemd-x86_64.tar.gz
# Extract just runtime binaries, initialization SQL/scripts and plugins; remove archive.
set -euo pipefail
umask 077
repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
base=${VDA_LOCAL_MYSQL_DIR:-$HOME/.local/share/vda-mysql}/mariadb
version=11.4.5
install="$base/mariadb-$version-linux-systemd-x86_64"
client() { "$install/bin/mariadb" --no-defaults --socket="$base/mariadb.sock" -uroot "$@"; }
running() { client -e 'SELECT 1' >/dev/null 2>&1; }
start() {
  mkdir -p "$base"
  if running; then printf 'MariaDB already running on 127.0.0.1:33307\n'; return; fi
  if [[ ! -x $install/bin/mariadbd ]]; then
    curl -fL --retry 2 -o "$base/mariadb.tar.gz" "https://archive.mariadb.org/mariadb-$version/bintar-linux-systemd-x86_64/mariadb-$version-linux-systemd-x86_64.tar.gz"
    tar -xzf "$base/mariadb.tar.gz" -C "$base" --wildcards '*/bin/mariadbd' '*/bin/mariadb' '*/bin/my_print_defaults' '*/bin/resolveip' '*/scripts/mariadb-install-db' '*/share/*' '*/lib/plugin/*'
    rm -- "$base/mariadb.tar.gz"
  fi
  if [[ ! -d $base/data/mysql ]]; then
    "$install/scripts/mariadb-install-db" --no-defaults --basedir="$install" --datadir="$base/data" --auth-root-authentication-method=normal > "$base/init.log" 2>&1
  fi
  local -a launch=(nohup)
  if command -v systemd-run >/dev/null && systemctl --user is-system-running >/dev/null 2>&1; then
    launch=(systemd-run --user --unit=vda-mariadb --collect)
  fi
  "${launch[@]}" "$install/bin/mariadbd" --no-defaults --basedir="$install" --datadir="$base/data" \
    --bind-address=127.0.0.1 --port=33307 --socket="$base/mariadb.sock" --pid-file="$base/mariadb.pid" \
    --skip-log-bin --innodb-buffer-pool-size=64M --innodb-log-file-size=32M --log-error="$base/mariadb.log" \
    > "$base/stdout.log" 2>&1 </dev/null &
  for ((attempt=0;attempt<100;attempt++)); do
    if running; then
      client <<'SQL'
CREATE USER IF NOT EXISTS 'vda'@'127.0.0.1' IDENTIFIED BY 'vda_local_pw';
GRANT ALL PRIVILEGES ON *.* TO 'vda'@'127.0.0.1' WITH GRANT OPTION;
CREATE DATABASE IF NOT EXISTS shop CHARACTER SET utf8mb4;
CREATE USER IF NOT EXISTS 'shop_reader'@'127.0.0.1' IDENTIFIED BY 'shop_reader_pw';
GRANT SELECT ON shop.* TO 'shop_reader'@'127.0.0.1';
CREATE USER IF NOT EXISTS 'shop_writer'@'127.0.0.1' IDENTIFIED BY 'shop_writer_pw';
GRANT SELECT,INSERT,UPDATE,DELETE ON shop.* TO 'shop_writer'@'127.0.0.1';
SQL
      printf 'MariaDB %s running on 127.0.0.1:33307\n' "$version"; return
    fi
    sleep 0.2
  done
  cat "$base/mariadb.log" >&2; exit 1
}
case ${1:-status} in
  start) start ;;
  status) if running; then client -e 'SELECT VERSION() AS version, @@port AS port'; else printf 'MariaDB is stopped\n'; exit 1; fi ;;
  stop) if running; then client -e SHUTDOWN; printf 'MariaDB stopped\n'; else printf 'MariaDB is stopped\n'; fi ;;
  seed)
    start
    if [[ $(client -N -e "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema='shop' AND table_name='customers'") == 0 ]]; then
      sed 's/cte_max_recursion_depth/max_recursive_iterations/' "$repo_dir/deploy/demo/mysql.sql" | client
    fi
    client -e 'SELECT COUNT(*) AS customers FROM shop.customers' ;;
  *) printf 'Usage: %s start|stop|status|seed\n' "$0" >&2; exit 2 ;;
esac
