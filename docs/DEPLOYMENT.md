# Deployment and operations

visp-db-access ships as one binary (`visp-db-access`) with the web console
embedded. Nodes are stateless; all state lives in a PostgreSQL metadata
database. Run as many replicas as you need behind any HTTP load balancer.

## Configuration

All settings are environment variables (each also has a matching CLI flag).
The binary does not load `.env` files itself; see [.env.example](../.env.example)
and [deploy/config.env.example](../deploy/config.env.example).

| Variable                       | Default        | Purpose                                                                                                                                                    |
| ------------------------------ | -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `VDA_DATABASE_URL`             | required       | PostgreSQL metadata URL. Migration privileges are needed on first start.                                                                                   |
| `VDA_MASTER_KEY`               | required       | Standard-base64 32-byte key that encrypts stored credentials. Generate with `openssl rand -base64 32` or `visp-db-access gen-key`.                         |
| `VDA_BIND`                     | `0.0.0.0:8080` | Listening address.                                                                                                                                         |
| `VDA_METADATA_POOL_SIZE`       | `20`           | Metadata connections per node (1–200). Health and discovery schedulers each use one more.                                                                  |
| `VDA_BOOTSTRAP_ADMIN_EMAIL`    | unset          | First administrator, created only when no users exist. Set with the password.                                                                              |
| `VDA_BOOTSTRAP_ADMIN_PASSWORD` | unset          | First administrator's password, at least 12 bytes.                                                                                                         |
| `VDA_ADMIN_PASSWORD`           | unset          | Password for the `create-admin` command.                                                                                                                   |
| `VDA_COOKIE_SECURE`            | `true`         | Secure session cookie and HSTS. Set `false` only for local plain-HTTP development (a startup warning is logged).                                           |
| `VDA_ALLOWED_CIDRS`            | empty          | Comma-separated client allowlist; intersected with the allowlist set in the console. Empty allows all.                                                     |
| `VDA_TRUST_PROXY_HEADERS`      | `false`        | Use `X-Forwarded-For` for client attribution.                                                                                                              |
| `VDA_TRUSTED_PROXIES`          | empty          | Comma-separated CIDRs of your proxies. With forwarding on and no list, only the direct peer is trusted. `VDA_TRUSTED_PROXY_CIDRS` is accepted as an alias. |
| `VDA_ALLOW_PRIVATE_TARGETS`    | `false`        | Allow loopback database targets (development). RFC 1918 targets are always allowed.                                                                        |
| `VDA_METRICS_TOKEN`            | unset          | When set, `/metrics` requires `Authorization: Bearer <token>`.                                                                                             |
| `VDA_LOG_FORMAT`               | `text`         | `text` or `json`.                                                                                                                                          |
| `RUST_LOG`                     | `info`         | Log filter.                                                                                                                                                |

Back up the master key together with the metadata database. Stored credentials
can't be decrypted with a different key; if the key is lost, re-enter each
cluster's password in its settings.

### Policy defaults

Each cluster has a safety policy, editable in the console. Defaults depend on
the environment:

|                      | Production | Other environments         |
| -------------------- | ---------- | -------------------------- |
| Maximum result rows  | 1,000      | 5,000                      |
| Statement timeout    | 15 s       | 60 s                       |
| Writes               | Blocked    | Allowed with a write grant |
| Writes need approval | Yes        | Yes                        |
| DDL                  | Blocked    | Blocked                    |

Both use a 2-second lock timeout, four concurrent queries per node, a
1,000-row affected-row cap and read-replica routing. Hard upper bounds are
100,000 rows, 10-minute timeouts and 32 MiB of result data.

## Commands

```sh
visp-db-access serve          # default; applies migrations, then serves
visp-db-access migrate        # apply metadata migrations only
visp-db-access gen-key        # print a new master key
VDA_ADMIN_PASSWORD=… visp-db-access create-admin --email you@example.com --name "Your Name"
```

Startup applies migrations and creates the bootstrap admin only if there are no
users, under an advisory lock so concurrent replicas don't race.

## Docker

The image builds the console and the Rust binary in separate stages and runs as
a non-root user on a distroless base.

```sh
docker build -t visp-db-access .
docker compose up --build            # gateway + metadata Postgres
docker compose --profile demo up     # plus demo PostgreSQL and MySQL targets
```

## Kubernetes (Helm)

`deploy/helm/visp-db-access` provides a two-replica Deployment, Service,
optional TLS Ingress, HorizontalPodAutoscaler and PodDisruptionBudget. Provide
an existing Secret with `database-url` and `master-key`, and optionally
`bootstrap-admin-email` and `bootstrap-admin-password`.

```sh
helm upgrade --install vda deploy/helm/visp-db-access \
  --set image.repository=your-registry/visp-db-access \
  --set existingSecret=vda-secrets
```

For AWS discovery on EKS, see [IRSA setup](DISCOVERY.md#eks-irsa).

## Health and metrics

- `/healthz` reports process liveness; `/readyz` checks the metadata database
  with a short deadline.
- `/metrics` exports Prometheus request counters, query durations, blocked
  queries and pool gauges. Restrict it at your ingress or set
  `VDA_METRICS_TOKEN`.
- Cluster health probes run every 30 seconds on one node (elected through a
  PostgreSQL advisory lock), with up to 16 probes in parallel. Health history is
  kept for seven days.

## Operations notes

- Shutdown is graceful: active queries are cancelled, HTTP requests and queued
  history drain, pools close and the scheduler lock is released.
- Each node keeps small per-cluster connection pools (up to five connections per
  endpoint). Budget target connections across replicas accordingly.
- Concurrency limits, cancellation and login throttling are per node. Use
  load-balancer affinity so a cancel request reaches the node running the query.
- Runtime network settings are cached for 30 seconds and schema trees for five
  minutes.
