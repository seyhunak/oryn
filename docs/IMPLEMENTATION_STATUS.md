# Implementation status

What ORYN does today and how each claim was checked. Three states only:

- **Verified**: exercised by an automated test in this repository or by a recorded manual
  run against the real services, as cited.
- **Implemented**: the code exists and compiles, its unit tests (if any) pass, but nothing
  has driven it end to end against a real external system.
- **Not implemented**: planned, absent. The UI never presents these as working.

Last full check: 2026-11-13.

## How to reproduce the checks

```bash
make test        # cargo test --workspace (73 tests incl. Postgres-backed e2e), runner, demo app, web
make demo        # seed + run every demo suite on a real worker and Chromium
node scripts/verify-demo.mjs   # asserts the outcome of every demo scenario
```

The reference run: `verify-demo` reported **6 suites, 26 scenarios passed, 4/4
planted-defect scenarios failed as expected**, executed by worker `local-01` through the
Playwright runner (Chromium) against `examples/demo-application`.

## Control plane

| Capability | State | Evidence |
| --- | --- | --- |
| Registration, login, sessions, logout, CSRF header on cookie writes | Verified | `crates/oryn-api/tests/e2e.rs` `vertical_slice` |
| Login rate limit per IP; account lockout counting failures only | Verified | `e2e.rs` `account_lockout_counts_failures_only` |
| Client IP from `X-Forwarded-For` only via trusted proxies | Verified | `auth::ip_tests` |
| Personal API tokens with a role ceiling | Verified | `vertical_slice` (token cannot exceed its ceiling) |
| RBAC: workspace roles, viewer cannot write | Verified | `vertical_slice`; `rbac::tests`; `role_matrix_matches_code` |
| OIDC single sign-on (authorization code + PKCE, ID token claims and nonce checked, verified email required) | Implemented | claim checks unit-tested (`id_token_claims_are_enforced`); not run against a real identity provider |
| Workspaces, projects (incl. private projects), environments, encrypted secrets | Verified | `vertical_slice` (private project hidden from a viewer; secret delivered only to the claiming worker) |
| Schema constraints match the values the code writes | Verified | `crates/oryn-db/tests/queue.rs` `checks_accept_values_the_code_writes` |
| Audit log | Verified | `vertical_slice` reads the audit trail |
| OpenAPI document and ReDoc reference at `/api/docs` | Verified | `docs/api/openapi.json` exported from the running API (101 paths, 131 operations) |

## Testing engines

| Capability | State | Evidence |
| --- | --- | --- |
| Gherkin compile with a closed step library, diagnostics, Scenario Outlines, tag inheritance | Verified | `oryn-bdd` unit tests; demo features compile and run |
| API engine: requests, JSON assertions, variables, secrets | Verified | `oryn-api-engine` unit tests; `api_*` demo suites |
| Browser steps over `oryn.browser/v1` | Verified | runner `test/*.test.ts` (26 tests); browser demo suites in Chromium |
| Exact-first target resolution (role, label, placeholder, test id, text) | Verified | `adapters/playwright-runner/test/targets.test.ts` |
| Evidence: per-step expected/actual, screenshots, logs, network, redaction | Verified | `runner/test/redaction.test.ts`; artifacts asserted in `vertical_slice` |
| Scenario approval gate (unapproved scenarios cannot run) | Verified | `vertical_slice` (422 before approval) |
| Browser recording into Gherkin with password redaction | Implemented | `routes/testing.rs` recordings; needs a runner with a display, not exercised in CI |

## Execution fabric

| Capability | State | Evidence |
| --- | --- | --- |
| Postgres job queue with `SKIP LOCKED` leases, lease expiry, retries as new attempts | Verified | `crates/oryn-db/tests/queue.rs` |
| Worker self-registration with bootstrap token, persisted token file | Verified | `crates/oryn-worker/tests/worker.rs` |
| Concurrent jobs per worker; heartbeat keeps declared concurrency | Verified | `worker.rs` (two scenarios in parallel, concurrency 2 reported) |
| Graceful drain on shutdown; token revocation stops the worker | Verified | `worker.rs` |
| Cancellation, re-run keeping suite and retry policy | Verified | `vertical_slice` (rerun asserts `suite_id`, `rerun_of`) |
| Correct execution roll-up under concurrent completions | Verified | demo run with parallel jobs; fixed by locking the execution row |
| Live progress over SSE | Verified | `vertical_slice` reads the stream |

## Intelligence

| Capability | State | Evidence |
| --- | --- | --- |
| Repository analysis (languages, frameworks, routes, tests, modules) from a local path | Verified | `vertical_slice`; `oryn-intelligence` unit tests |
| Git, GitHub, GitLab clone and ZIP import with archive validation | Implemented | unit tests for ZIP validation and SSRF guard; remote clones not run in CI |
| Requirements import and tag-based traceability | Verified | `vertical_slice`; seeded demo project |
| Heuristic (no-AI) test planner, edit, approve, reject | Verified | `e2e.rs` `heuristic_planner_and_approval` |
| AI planner through the model gateway, usage recording | Verified (mock provider) | `heuristic_planner_and_approval` with the Mock provider |
| Failure investigation: deterministic classification, observations kept apart from interpretation | Verified | `vertical_slice`; `investigator` unit tests |
| Regression selection with scored reasons | Verified | `vertical_slice`; `regression` unit tests |
| Metrics, trends, flakiness, coverage, recommendations | Verified | `vertical_slice` analytics reads; formulas in [METRICS.md](METRICS.md) |

## Model gateway

| Provider | State | Evidence |
| --- | --- | --- |
| Mock | Verified | used by the e2e tests and the demo seed |
| Anthropic, OpenAI, OpenAI-compatible | Implemented | wire-format unit tests (`providers::tests`); no live calls in CI |
| Azure OpenAI, Gemini, Ollama | Implemented | compiled and wired in `providers.rs`; no tests against these APIs yet |
| Prompt-injection fencing (all user and repository text, tags defused in any spelling), schema-validated output with one repair attempt, redaction | Verified | `oryn-model-gateway` tests `fencing`, `sanitizes`, `invalid_output_gets_one_repair`, `retries_redacts_and_costs`; `planner::tests::ai_plan_is_validated` |

## Security controls

| Control | State | Evidence |
| --- | --- | --- |
| SSRF guard on outbound URLs (private ranges blocked unless allowed) | Verified | `oryn-api-engine` `policy::tests`; `import::tests::git_url_policy` |
| Test traffic limited to environment hosts (wildcards, every redirect hop, recording targets), metadata hosts always blocked | Verified | `redirects_to_disallowed_hosts_are_blocked`; runner `redaction.test.ts` |
| DNS-rebinding protection for webhooks, Slack, repository hosts and AI providers (resolve once, check, pin) | Verified | `oryn-auth` `net::tests` (a name resolving to loopback is refused at connect time); `clone_refuses_names_that_resolve_to_private_addresses`; manual clones showed git honours the pinned address (git 2.39 and 2.56) |
| ZIP validation (size, compression ratio, traversal, symlinks) | Verified | `oryn-intelligence` `import::tests::rejects_traversal_and_bombs` |
| Credentials encrypted at rest (AES-256-GCM) | Verified | `oryn-auth` `tests::secret_box`; secret round-trip in `vertical_slice` |
| Secrets redacted from step output and evidence | Verified | `runner::tests::secrets_are_redacted`, `redact::tests`, runner `redaction.test.ts` |
| Tenant isolation between workspaces | Verified | `crates/oryn-db/tests/queue.rs` `workspace_isolation` |
| A model cannot mark a test passed | Verified by design and test | verdicts are computed only in `oryn-execution`; `vertical_slice` checks failed counts come from assertions |

## Web application

| Area | State | Evidence |
| --- | --- | --- |
| Sign-in, registration, app shell, workspace switcher | Verified | manual run in the browser against the live API |
| Overview | Verified | manual run, real data from the seeded project |
| Executions list, execution detail with live updates, job evidence tabs | Verified | manual run against a real demo execution |
| Projects: list, create, overview, requirements (CRUD, import, link), traceability, repository import and context, environments with secrets, test data, access grants, settings | Verified | manual run against the live API, each create/edit/delete exercised |
| Test Studio: suite editor (Monaco, live validation, step library), scenario approval, run dialog, custom steps | Verified | manual run; the approval gate's 422 is shown inline |
| Test Studio: browser recording | Implemented | UI handles the "recorder not configured" case; a live recording was not run |
| AI Test Planner: heuristic and AI (mock) plans, item edit and re-validation, approve, reject | Verified | manual run |
| Quality: health, flaky, failures, recommendations, regression selection | Verified | manual run; "Run selected" from regression not exercised in the UI |
| Execution compare | Verified | manual run |
| Workers: list, detail, add (token shown once), revoke | Verified | manual run (a worker created and revoked) |
| Integrations: webhook and Slack create, test, deliveries, delete | Verified | manual run (webhook) |
| Settings: workspace, members, teams with membership, API tokens, AI providers and usage, repository connections, audit log | Verified | manual run; team membership also asserted in `vertical_slice` |
| Responsive layout: desktop (1280×800), narrow pane (800 px), phone (375 px) | Verified | screenshots of every top-level screen; no horizontal page scroll at 375 px |
| Web unit tests, lint, type check | Verified | `pnpm test` (8), `pnpm lint` (ESLint + `tsc`) |

## Delivery

| Item | State | Evidence |
| --- | --- | --- |
| Docker images (api, worker, web, runner, demo) and Compose `app` profile | Verified | fresh `docker compose --profile app up --build --wait`: every service healthy, then `seed.mjs --run` and `verify-demo.mjs` passed (6 suites, 26 scenarios, 4/4 planted defects caught) |
| Kubernetes manifests | Implemented | `kubectl kustomize infrastructure/kubernetes` renders; not applied to a cluster |
| GitHub Actions CI | Implemented | workflow written; has not run on GitHub yet |
