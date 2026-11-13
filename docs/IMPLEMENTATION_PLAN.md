# Implementation plan

Phases 1 and 2 are built; their scope below describes what exists. Later phases are not implemented. The current state of each capability, with the test that verifies it, is tracked in [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md).

## Phase 1: vertical slice (built)

Goal: one person can write a Gherkin scenario, approve it, run it on a self-hosted worker against a real application, and read the evidence.

Scope:

- Rust workspace: domain types and RBAC (`oryn-domain`), auth primitives (`oryn-auth`), PostgreSQL repository and migrations (`oryn-db`), Gherkin compiler and step library (`oryn-bdd`), API engine (`oryn-api-engine`), execution state machine and scenario runner (`oryn-execution`), artifact storage (`oryn-artifacts`), tracing setup (`oryn-observability`), HTTP API (`oryn-api`), worker (`oryn-worker`).
- Accounts, sessions with a CSRF header, API tokens with a role ceiling, workspaces, members, projects, environments with encrypted secrets, audit log.
- PostgreSQL job queue with `SKIP LOCKED` leases, lease expiry, retries as new attempts, cancellation, re-runs.
- Worker registration (bootstrap and admin-created), heartbeats, concurrent jobs, graceful drain.
- Playwright runner over `oryn.browser/v1` with network policy and redaction.
- Next.js web app: projects, Test Studio, executions with live progress and evidence, workers, settings.
- Demo application with planted defects, seed script and `verify-demo.mjs`.
- OpenAPI generated from handlers; web types generated from it.

Exit criteria (met):

- `crates/oryn-api/tests/e2e.rs` `vertical_slice` passes: register, create project and environment with a secret, save a suite, approval gate returns `422` before approval, run, worker claims and completes, evidence and audit are readable, re-run keeps suite and retry policy.
- `crates/oryn-db/tests/queue.rs` and `crates/oryn-worker/tests/worker.rs` pass.
- The CI `e2e` job runs every demo suite in Docker Compose and `verify-demo.mjs` confirms the expected passes and the planted-defect failures.

## Phase 2: intelligence (built)

Goal: help people decide what to test and why something failed, without letting a model decide outcomes.

Scope:

- Repository import (GitHub, GitLab, git URL, ZIP, local directory) with safe extraction and cloning; static analysis of languages, frameworks, dependencies, routes, tests and modules; change sets between analyses; full-text search over redacted chunks.
- Requirements import from Markdown, manual and tag-based links, traceability matrix.
- Model gateway (`oryn-model-gateway`) with Anthropic, OpenAI, Azure OpenAI, Gemini, Ollama, OpenAI-compatible and mock providers; structured outputs, one schema repair, retries, redaction, untrusted-content fencing; usage and cost records.
- Test planner: heuristic and AI modes, review per item, approval into suites.
- Deterministic failure investigator with optional, labelled AI interpretation.
- Quality metrics, flakiness, deterministic recommendations, regression selection by changed files.
- Webhook and Slack notifications with signed payloads and a delivery log.
- OIDC sign-in.
- Browser recording into Gherkin.

Exit criteria (met for everything except where noted in IMPLEMENTATION_STATUS.md):

- Unit tests in `oryn-intelligence` and `oryn-model-gateway` pass with the mock provider; CI needs no AI credentials.
- The planner, investigator and regression selection work with no provider configured.
- OIDC and recording are implemented but not exercised against a real identity provider or a headed runner.

## Phase 3: hardening (not implemented)

Goal: close the gaps found while documenting the code. Each item is a bug or a known limitation from [SECURITY.md](SECURITY.md#known-limitations) and [DATA_MODEL.md](DATA_MODEL.md#discrepancies).

| Item | Exit criterion | Status |
|---|---|---|
| Project visibility: the `projects.visibility` check allowed `restricted`, but the API writes `private`. | Creating a private project succeeds and a member without a grant cannot see it; covered by an e2e test. | Done: migration `0002`; `vertical_slice` |
| `test_plan_items.category` check lacked `smoke` and `accessibility`, which the planners produce. | Every planner category satisfies the check. | Done: migration `0002`; `checks_accept_values_the_code_writes` |
| Default integration events (`execution.completed`) did not match the emitted events. | A new integration with default events receives deliveries. | Done: default is `execution.finished`; same test |
| Allowed hosts: base URL hosts were dropped when `allowed_hosts` was set; the runner ignored `*.domain` wildcards. | Rust and runner policies follow one rule set. | Done: runner `redaction.test.ts`, `policy::tests` |
| Test-step redirects not re-checked; runner blocked one metadata address; recording targets unchecked. | Every redirect hop and every recording target is checked against the environment's hosts. | Done: `redirects_to_disallowed_hosts_are_blocked`; recording uses the environment hosts |
| SSRF by DNS rebinding (webhooks) and unresolved git / AI `base_url` hosts. | One resolver that pins the checked address for the connection is used by every outbound path. | Done: `oryn_auth::net` (`guarded_client_refuses_private_destinations`); git pinned via `http.curloptResolve` (`clone_refuses_names_that_resolve_to_private_addresses`) |
| Forwarded IP was trusted unconditionally. | A trusted-proxy setting; `X-Forwarded-For` is ignored from other peers. | Done: `ORYN_TRUSTED_PROXIES`; `forwarded_for_is_only_believed_from_trusted_peers` |
| OIDC: ID token not validated, nonce unused, missing `email_verified` accepted, registration setting bypassed. | Issuer, audience, expiry and nonce validated; linking needs a verified email; `ORYN_ALLOW_REGISTRATION=false` blocks new OIDC accounts. | Done (signature check replaced by TLS to the token endpoint, OIDC Core 3.1.3.7): `id_token_claims_are_enforced`; not yet run against a live provider |
| Generated tokens contained a double underscore (`oryn_pat__…`). | Tokens have a single separator; existing tokens keep working. | Done: `oryn-auth` `tokens` |
| The planner and investigation shared the `ai:{user}` rate-limit counter. | Separate keys per purpose. | Done |
| Plan approval was not atomic. | Concurrent approvals cannot both run, and a failure part-way leaves no suites behind. | Done: the plan is claimed with a conditional update, created suites are deleted on failure |
| The SSE doc comment mentioned step events that are not emitted. | The comment is correct. | Done |
| Prompt fencing was case-sensitive and missed some planner context. | All user and repository text is fenced; fence tags are defused in any spelling. | Done: gateway `fencing` test |

## Phase 4: scheduling and integrations (not implemented)

| Item | Exit criterion |
|---|---|
| Scheduled executions | A schedule (cron expression, suite or tags, environment) creates executions with trigger `schedule`; missed runs are not duplicated across API replicas. |
| Issue trackers (Jira, GitHub, GitLab) | A failed execution can create or update an issue with a link to the evidence; credentials are stored encrypted. |
| CI providers (GitHub Actions, GitLab CI) | A check or commit status reflects the execution result for its `commit_sha`. |
| Worker label routing | A job carries required labels and is claimed only by workers that have them; browser type is matched to declared capabilities. |
| Workspace-isolated secrets for shared workers | Shared workers receive secrets only for workspaces that opt in. |

## Phase 5: scale and retrieval (not implemented)

| Item | Exit criterion |
|---|---|
| Object storage for artifacts | An S3-compatible backend selected by configuration; `artifacts.storage_backend` reflects it; existing local artifacts stay readable. |
| Vector retrieval | Code chunks are embedded through the gateway; planner context combines full-text and vector search; works with embeddings disabled. |
| AI recommendations | Model-proposed recommendations are stored with a distinct source and never replace deterministic ones. |
| Composite step definitions | Custom step definitions are expanded by the compiler, with cycle detection and per-step evidence. |
| Metrics endpoint | A Prometheus endpoint for queue depth, job durations, worker counts and AI usage. |
| Secret key rotation | A command re-encrypts all secrets under a new key without downtime. |
