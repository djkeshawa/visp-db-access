#!/usr/bin/env bash
# Local development only: binds loopback and permits loopback database targets.
set -euo pipefail
umask 077
repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_dir"
export VDA_DATABASE_URL=${VDA_DATABASE_URL:-postgres://vda@127.0.0.1:55432/vda_meta}
export VDA_BIND=${VDA_BIND:-127.0.0.1:8080}
export VDA_COOKIE_SECURE=${VDA_COOKIE_SECURE:-false}
export VDA_ALLOW_PRIVATE_TARGETS=${VDA_ALLOW_PRIVATE_TARGETS:-true}
export VDA_BOOTSTRAP_ADMIN_EMAIL=${VDA_BOOTSTRAP_ADMIN_EMAIL:-admin@local.test}
export VDA_BOOTSTRAP_ADMIN_PASSWORD=${VDA_BOOTSTRAP_ADMIN_PASSWORD:-local-admin-password}

# Build the SPA before Rust compilation so rust-embed picks up current assets.
npm --prefix web run build
build_args=(-p vda-server)
if [[ -n ${VDA_DISCOVERY_FAKE_FIXTURE:-} ]]; then
  if [[ ! -r $VDA_DISCOVERY_FAKE_FIXTURE ]]; then
    printf 'Discovery fixture is not readable: %s\n' "$VDA_DISCOVERY_FAKE_FIXTURE" >&2
    exit 1
  fi
  build_args+=(--features fake-discovery)
  printf 'WARNING: enabling fake cloud discovery for local development.\n' >&2
fi
cargo build "${build_args[@]}"
server_bin="${CARGO_TARGET_DIR:-target}/debug/visp-db-access"
if [[ -z ${VDA_MASTER_KEY:-} ]]; then
  # Keep a stable key across restarts; changing it makes stored credentials unreadable.
  # Outside build caches so cleaning target dirs never orphans stored credentials.
  key_dir="$repo_dir/.dev"
  mkdir -p -- "$key_dir"
  key_file="$key_dir/master-key"
  if [[ ! -s $key_file ]]; then
    "$server_bin" gen-key > "$key_file"
  fi
  export VDA_MASTER_KEY
  VDA_MASTER_KEY=$(cat -- "$key_file")
fi
printf 'Starting local gateway at %s (Ctrl+C to stop).\n' "$VDA_BIND"
exec "$server_bin" serve
