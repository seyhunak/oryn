# Deployment

ORYN is five processes plus two data stores:

| Component | Image / build | Stateful |
|---|---|---|
| API (`oryn-api`) | `ghcr.io/seyhunak/oryn-api`, [`infrastructure/docker/api.Dockerfile`](../infrastructure/docker/api.Dockerfile) | Artifacts and repository working copies under `/data` |
| Worker (`oryn-worker`) | `ghcr.io/seyhunak/oryn-worker`, [`worker.Dockerfile`](../infrastructure/docker/worker.Dockerfile) | Token file under `/state` (self-registered workers only) |
| Playwright runner | `ghcr.io/seyhunak/oryn-playwright-runner`, [`adapters/playwright-runner/Dockerfile`](../adapters/playwright-runner/Dockerfile) | No |
| Web (Next.js) | `ghcr.io/seyhunak/oryn-web`, [`web.Dockerfile`](../infrastructure/docker/web.Dockerfile) | No |
| PostgreSQL 15+ with pgvector | `pgvector/pgvector:pg17` in Compose | Yes |
| Redis 7 (optional) | `redis:7-alpine` | No (rate-limit counters only) |

The API image also contains `oryn-migrate`. The API and worker images run as uid 10001; the runner runs as `pwuser`.

## Docker Compose

[`docker-compose.yml`](../docker-compose.yml) defines all services. Profiles:

| Profile | Services |
|---|---|
| none | `postgres`, `redis` (what `make deps` starts for local development) |
| `app` | adds `api`, `worker`, `playwright-runner`, `web`, `demo-app` |
| `demo` | `demo-app` only |

```bash
make up      # docker compose --profile app up -d --build --wait, then seeds the demo project
make logs
make down    # keeps volumes pgdata, oryndata, workerstate
```

Compose publishes only to `127.0.0.1`: web on 3000, API on 8080, demo app on 4000, Postgres on 55433, Redis on 56380. The runner has no published port; workers reach it at `http://playwright-runner:7400`. The Compose file is configured for evaluation: `ORYN_COOKIE_SECURE=false`, `ORYN_ALLOW_PRIVATE_NETWORK=true`, and development defaults for the secret key, bootstrap token and runner token. Do not expose it as-is.

**The web image bakes in the API address.** Next.js resolves the `/api` rewrite at build time from the build argument `ORYN_API_INTERNAL_URL` (Compose uses `http://api:8080`). Rebuild the web image if the API's internal address changes.

## Kubernetes

[`infrastructure/kubernetes/`](../infrastructure/kubernetes) is a Kustomize base (`kubectl apply -k infrastructure/kubernetes`) with a namespace, an example secret, a config map, in-cluster Postgres and Redis, the API (with an `oryn-migrate` init container and a persistent volume for `/data`), workers, runners, web and an ingress. Its own comments describe it as evaluation-grade: replace the secret, use managed Postgres with pgvector, terminate TLS at the ingress and set `ORYN_COOKIE_SECURE=true`.

## Production checklist

**Secrets**

- [ ] `ORYN_SECRET_KEY`: at least 32 random characters (`openssl rand -base64 48`). Back it up; losing or changing it makes every stored secret, repository token and AI key unreadable. There is no rotation command.
- [ ] `ORYN_WORKER_BOOTSTRAP_TOKEN`: random, or unset to allow only admin-created workers.
- [ ] `ORYN_RUNNER_TOKEN` on workers and `RUNNER_TOKEN` on runners: the same random value.
- [ ] `ORYN_RECORDER_TOKEN` matches the runner's token if recording is enabled.

**Network**

- [ ] Serve the web app over TLS and set `ORYN_COOKIE_SECURE=true` and `ORYN_PUBLIC_URL` to the public origin.
- [ ] If a reverse proxy sits in front of the API on a public address, add it to `ORYN_TRUSTED_PROXIES`. By default only loopback and private-range peers may set `X-Forwarded-For`; every other peer's header is ignored.
- [ ] Do not expose the API's worker endpoints or the runner to the internet. Runners need to be reachable only from workers.
- [ ] Keep `ORYN_ALLOW_PRIVATE_NETWORK=false` unless webhooks, git servers or targets are on private networks. Note that test steps are governed by each environment's allowed hosts, not by this flag.
- [ ] Leave `ORYN_CORS_ORIGINS` unset when the UI is served through the web app's proxy (same origin).

**Accounts**

- [ ] Register the first account (it becomes instance admin), then set `ORYN_ALLOW_REGISTRATION=false` if accounts should only be created by invitation. OIDC sign-in still creates accounts when it is configured.
- [ ] For OIDC, set `ORYN_OIDC_ISSUER`, `ORYN_OIDC_CLIENT_ID`, `ORYN_OIDC_CLIENT_SECRET` and register `${ORYN_PUBLIC_URL}/api/v1/auth/oidc/callback` as the redirect URI. Use an identity provider that sends `email_verified`.

**Data**

- [ ] PostgreSQL 15 or newer with the `vector` and `pg_trgm` extensions available (the first migration creates them). Back it up.
- [ ] Persistent storage for `ORYN_ARTIFACT_DIR` and `ORYN_WORKSPACE_DIR` (default `/data/artifacts`, `/data/repos`). Back up the artifact directory with the database; artifact rows point to files there. Artifacts are not encrypted at rest.
- [ ] Size limits: `ORYN_MAX_ARTIFACT_BYTES` (default 100 MiB per artifact), `ORYN_MAX_UPLOAD_BYTES` (default 200 MiB per request).
- [ ] `ORYN_LOCAL_IMPORT_ROOT`: unset in production unless you want users to import from a directory on the API host.

## Migrations

The API runs pending migrations on start. To run them separately (recommended before a rolling update), run `oryn-migrate` from the API image with the same `DATABASE_URL`; the Kubernetes base does this in an init container. Migrations are forward-only.

## Scaling

**Workers** scale horizontally. Each claims jobs with `FOR UPDATE SKIP LOCKED`, so any number can poll the same database through the API. Per worker:

- `ORYN_WORKER_CONCURRENCY` (default 2, 1–64): parallel jobs.
- `ORYN_RUNNER_URL`: required for browser and mixed jobs; without it the worker claims only API jobs.
- `ORYN_WORKER_POLL_MS` (default 2000): idle poll interval.
- Identity: either `ORYN_WORKER_TOKEN` (a workspace worker created in the UI, recommended for multi-tenant instances) or bootstrap self-registration (a shared worker that serves every workspace and stores its token in `ORYN_WORKER_TOKEN_FILE`). Shared workers receive secrets from every workspace whose jobs they run.
- On `SIGTERM` a worker stops claiming and finishes running jobs. Jobs of a worker that disappears are retried after the lease (`ORYN_JOB_LEASE_SECS`, default 60) expires.

`ORYN_WORKER_LABELS` is reported to the API but not used for routing.

**Runners** are stateless. `MAX_CONCURRENCY` (default 2) caps browsers per runner; they need `--shm-size=1g` or `--ipc=host`. Point several workers at one runner, or run one runner beside each worker.

**API** replicas share all state through PostgreSQL. With more than one replica:

- Configure `REDIS_URL`; otherwise each replica has its own rate-limit counters.
- `ORYN_ARTIFACT_DIR` and `ORYN_WORKSPACE_DIR` must be on shared (ReadWriteMany) storage, since artifacts are written and read by whichever replica handles the request. Object storage is not implemented.
- Each replica runs the background tasks (lease reaper, stale-worker marker, session purge); they are idempotent.

The database pool is 20 connections per API replica.

## Observability

- **Logs**: structured `tracing` output; `ORYN_LOG_FORMAT=json` for log shippers, level via `RUST_LOG`. Each request is logged with method, path and `x-request-id`.
- **Traces**: set `OTEL_EXPORTER_OTLP_ENDPOINT` to export spans over OTLP.
- **Health**: `GET /healthz` (liveness: status, version, uptime) and `GET /readyz` (database and Redis reachability; `503` when the database is down). The runner exposes `GET /v1/health`.
- **Metrics**: no Prometheus endpoint exists. Queue depth is available at `GET /api/v1/workspaces/{ws}/queue`, worker state at `GET /api/v1/workspaces/{ws}/workers`, AI usage at `GET /api/v1/workspaces/{ws}/ai/usage`.

## Upgrades

1. Back up PostgreSQL and the artifact directory.
2. Run `oryn-migrate` from the new API image.
3. Roll the API, then workers and runners, then the web app. Workers drain on `SIGTERM`.
