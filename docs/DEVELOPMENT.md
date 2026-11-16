# Development

## Prerequisites

| Tool | Version |
|---|---|
| Rust | 1.85 or newer (`rust-version` in [`Cargo.toml`](../Cargo.toml)) |
| Node.js | 22 or newer |
| pnpm | 10 (web app) |
| npm | bundled with Node (runner and demo app) |
| Docker with Compose | for PostgreSQL (with pgvector) and Redis |
| git | used by the API for repository imports |

## First run

```bash
cp .env.example .env      # optional; the Makefile has working development defaults
make install              # cargo fetch, web and runner dependencies, Chromium for Playwright
make dev                  # Postgres + Redis in Docker, then API, runner, demo app, worker and web
make seed                 # in another terminal: demo account, project, suites, mock AI provider
```

Open http://localhost:3000 and sign in with `admin@oryn.local` / `oryn-demo-password` (the seed defaults; override with `ORYN_SEED_EMAIL` and `ORYN_SEED_PASSWORD`). `make demo` seeds and also starts one execution per demo suite.

`make dev` runs every process in one terminal and stops them all on Ctrl-C. To run them separately, use `make deps` and then `make dev-api`, `dev-worker`, `dev-runner`, `dev-web`, `dev-demo`.

The API applies migrations on start. To run them alone: `make migrate` (`cargo run -p oryn-db --bin oryn-migrate`).

## Local ports

| Service | Port | Started by |
|---|---|---|
| Web app | 3000 | `dev-web` |
| API | 8080 (bound to 127.0.0.1) | `dev-api` |
| Playwright runner | 7400 | `dev-runner` |
| Demo application | 4000 | `dev-demo` (`DEMO_FLAKY=1` enables its intentionally flaky endpoint) |
| PostgreSQL | 55433 on 127.0.0.1 | `deps` (`ORYN_PG_PORT` to change) |
| Redis | 56380 on 127.0.0.1 | `deps` |

## Configuration

The [`Makefile`](../Makefile) includes `.env` if present and otherwise supplies development defaults: `DATABASE_URL=postgres://oryn:oryn@localhost:55433/oryn`, a fixed development `ORYN_SECRET_KEY`, `ORYN_WORKER_BOOTSTRAP_TOKEN=dev-worker-bootstrap-token`, `ORYN_LOCAL_IMPORT_ROOT=<repo>/examples` and `ORYN_ALLOW_PRIVATE_NETWORK=true`. Data lives under `./data` (`artifacts`, `repos`, `worker-token`).

Every variable is documented in [`.env.example`](../.env.example). The ones that matter most while developing:

| Variable | Effect |
|---|---|
| `RUST_LOG` | Log filter, e.g. `info,oryn=debug`. |
| `ORYN_LOG_FORMAT` | `pretty` (default) or `json`. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Sends traces to an OTLP collector when set. |
| `ORYN_ALLOW_PRIVATE_NETWORK` | Must be `true` to test against `localhost` targets, webhooks or git servers. |
| `ORYN_RECORDER_URL`, `ORYN_RECORDER_TOKEN` | Where the API sends browser-recording requests (the runner). |

AI providers are not configured by environment; add them per workspace in Settings → AI models. The seed script registers the deterministic `mock` provider.

**Browser recording** opens a headed browser. It works when the runner runs on a machine with a display (`make dev-runner` on a desktop). The runner container has no display and returns `501 no_display`.

## Tests

```bash
make test          # all of the below
make test-rust     # starts Postgres, then cargo test --workspace --locked
make test-web      # apps/web: eslint + tsc, then vitest
make test-runner   # adapters/playwright-runner: node --test
make test-demo     # examples/demo-application
make lint          # cargo fmt --check, cargo clippy -D warnings, web lint
```

Database-backed Rust tests use `#[sqlx::test]`, which creates an isolated database per test from `DATABASE_URL` and runs the migrations into it. The main ones:

- [`crates/oryn-db/tests/queue.rs`](../crates/oryn-db/tests/queue.rs): claims, leases, expiry and retries.
- [`crates/oryn-api/tests/e2e.rs`](../crates/oryn-api/tests/e2e.rs): the vertical slice through the HTTP router, RBAC, lockout, token ceilings.
- [`crates/oryn-worker/tests/worker.rs`](../crates/oryn-worker/tests/worker.rs): a real worker against a real API, registration, concurrency, drain, revocation.

No test needs AI credentials; AI paths use the mock provider.

The end-to-end check that CI runs:

```bash
make up                         # whole stack in Docker Compose, then seed
node scripts/seed.mjs --run     # one execution per suite
node scripts/verify-demo.mjs    # asserts each demo scenario's expected outcome
```

## OpenAPI and web types

The OpenAPI document is generated from `#[utoipa::path]` annotations on the handlers. After changing a route or a request/response type:

```bash
make dev-api      # in one terminal
make openapi      # writes docs/api/openapi.json, then regenerates apps/web/lib/api/schema.d.ts
```

Commit both files. CI regenerates the web types from `docs/api/openapi.json` and fails if `schema.d.ts` differs.

## Repository layout

| Path | Contents |
|---|---|
| [`crates/`](../crates) | Rust workspace (12 crates; see [ARCHITECTURE.md](ARCHITECTURE.md#rust-crates)). Migrations are in `crates/oryn-db/migrations`. |
| [`apps/web/`](../apps/web) | Next.js web app. `app/(app)/` holds the nine sections; `lib/api/` the typed client. |
| [`apps/desktop/`](../apps/desktop) | macOS app (Tauri): a native window onto an ORYN server. See [DESKTOP.md](DESKTOP.md). |
| [`adapters/playwright-runner/`](../adapters/playwright-runner) | Node service that executes browser steps and recordings. |
| [`examples/demo-application/`](../examples/demo-application) | Demo shop with features and planted defects, used by the seed script and CI. |
| [`scripts/`](../scripts) | `seed.mjs`, `verify-demo.mjs`. |
| [`infrastructure/`](../infrastructure) | Dockerfiles and Kubernetes manifests. |
| [`docs/`](.) | This documentation, the OpenAPI document and the browser protocol. |

## Conventions

- `cargo fmt` formatting; `cargo clippy --workspace --all-targets --locked -- -D warnings` must pass. [`clippy.toml`](../clippy.toml) raises `too-many-arguments-threshold` to 14 and `type-complexity-threshold` to 600 because repository functions mirror table columns.
- SQL uses `sqlx` runtime queries; migrations are append-only files in `crates/oryn-db/migrations`.
- Errors returned from handlers use `ApiError`; internal errors are logged and returned as `internal error`.
- Every new endpoint needs a `utoipa::path` annotation, a permission check through `ws_ctx` or `project_ctx`, and an `audit` call if it changes state that an administrator would want to trace.
- Web components use design tokens from `apps/web/styles/tokens.css`; see [`design.md`](../design.md).
