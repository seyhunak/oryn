# API specification

The REST API is served by `oryn-api` (default port 8080). The OpenAPI 3.1 document is generated from the route handlers with `utoipa`, so the served document and the code cannot drift:

- Live document: `GET /api/openapi.json`
- Rendered reference (ReDoc): `GET /api/docs`
- Checked-in copy: [`docs/api/openapi.json`](api/openapi.json) (regenerate with `make openapi`, see [DEVELOPMENT.md](DEVELOPMENT.md#openapi-and-web-types))

This page covers conventions and an inventory; request and response schemas are in the OpenAPI document. The worker protocol is described in [ARCHITECTURE.md](ARCHITECTURE.md#job-protocol-orynjobv1).

## Conventions

### Base path

All application endpoints are under `/api/v1`. Health endpoints (`/healthz`, `/readyz`) and the documentation endpoints are outside it. Through the web UI (port 3000) the same paths are reachable on the UI origin, because Next.js proxies `/api/*` and `/healthz` to the API.

### Authentication

| Caller | Credential | Notes |
|---|---|---|
| Browser user | `oryn_session` cookie, set by `POST /auth/login`, `/auth/register` or the OIDC callback | `HttpOnly`, `SameSite=Lax`, `Path=/`, `Secure` when `ORYN_COOKIE_SECURE=true`. |
| Automation | `Authorization: Bearer oryn_pat_…` | Created under `POST /workspaces/{ws}/tokens`. Scoped to one workspace; effective role is `min(token role, member role)`. A request to another workspace returns `404`. |
| Worker | `Authorization: Bearer oryn_wrk_…` | Only accepted on `/api/v1/worker/*`. Revoked workers get `401`. |

**CSRF.** A cookie-authenticated request whose method is not `GET`, `HEAD` or `OPTIONS` must include the header `x-oryn-csrf` (any value, conventionally `1`), or it is rejected with `403 forbidden` ("missing CSRF header"). Bearer-token requests do not need it.

**CORS.** Disabled unless `ORYN_CORS_ORIGINS` lists origins; then those origins may send credentials with `Content-Type`, `Authorization` and `x-oryn-csrf`.

### Authorisation

Each endpoint requires a permission from the role matrix (`GET /api/v1/roles` returns it):

| Permission | owner | admin | engineer | viewer |
|---|:-:|:-:|:-:|:-:|
| `workspace:read`, `project:read` | yes | yes | yes | yes |
| `project:write`, `test:write`, `test:approve`, `execution:run`, `execution:cancel`, `ai:use` | yes | yes | yes | no |
| `project:delete`, `members:manage`, `worker:manage`, `ai:configure`, `integration:manage`, `secrets:manage`, `audit:read` | yes | yes | no | no |
| `workspace:manage` | yes | no | no | no |

Project endpoints use the caller's effective *project* role (see [SECURITY.md](SECURITY.md#rbac-and-tenant-isolation)). Resources the caller cannot see return `404`, not `403`.

### Errors

Every error has the shape:

```json
{ "error": { "code": "not_found", "message": "project not found" } }
```

| HTTP | `code` |
|---|---|
| 400 | `bad_request` |
| 401 | `unauthorized` |
| 403 | `forbidden` |
| 404 | `not_found` |
| 409 | `conflict` (unique violations, invalid state transitions) |
| 422 | `unprocessable` (valid request that cannot be acted on, e.g. no runnable scenarios) |
| 429 | `rate_limited` |
| 503 | `unavailable` (model call failed, recorder unreachable, OIDC discovery failed) |
| 500 | `internal` (message is always `internal error`; details are logged) |

Request bodies are limited to `ORYN_MAX_UPLOAD_BYTES` (default 200 MiB) and requests time out after 300 s (`408`). Every response carries `x-request-id`.

### Pagination

Paginated list endpoints (`GET /workspaces/{ws}/executions`, `GET /workspaces/{ws}/audit`) take `page` (1-based, default 1) and `per_page` (default 25, clamped to 1–200) and return:

```json
{ "items": [ ... ], "total": 132, "page": 1, "per_page": 25 }
```

Other list endpoints return a plain JSON array.

### Server-sent events

`GET /api/v1/executions/{eid}/events?after=<event id>` streams `text/event-stream`:

| Event | `data` |
|---|---|
| `job` | A `job_events` row: `{id, job_id, execution_id, kind, data, created_at}`; the SSE `id` is the row id, so `after` resumes. `kind` is one of `job.queued`, `job.claimed`, `job.started`, `job.finished`, `job.retry_scheduled`, `job.cancelled`, `job.cancel_requested`, `job.lease_expired`. |
| `execution` | `{status, total, passed, failed, errored, cancelled}` after each poll. |
| `end` | The final execution status; the stream then closes. |

The server polls every 0.8 s, sends keep-alives every 15 s, and closes after the execution is terminal with no new events (or after about two hours).

### Rate limits

Fixed windows, shared through Redis when configured, otherwise per process.

| Key | Limit |
|---|---|
| `POST /auth/login` per client IP | 30 per 5 minutes |
| Failed logins per email (lockout) | 10 failures per 15 minutes; only failures count |
| `POST /auth/register` per client IP | 10 per 10 minutes |
| `POST /worker/register` (global) | 60 per minute |
| AI planning per user (`ai:{user}`) | 30 per hour |
| AI investigation per user (`ai:{user}`, same counter) | 60 per hour |
| AI provider test per workspace | 20 per hour |

## Endpoint inventory

Generated from `docs/api/openapi.json` (130 operations). "Permission" is what the handler checks; "member" means any workspace member, "authenticated" means any signed-in user or token. Paths are relative to `/api/v1` unless they start with `/healthz` or `/readyz`.

### system

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/healthz` | Liveness: status, version, uptime. | none |
| GET | `/readyz` | Readiness: database and Redis reachability; `503` when the database is down. | none |

### auth

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/auth/providers` | Password/OIDC availability, whether registration is open, whether first-run setup is needed. | none |
| POST | `/auth/register` | Create an account and its first workspace; sets the session cookie. The first account becomes instance admin. | none (closed when `ORYN_ALLOW_REGISTRATION=false` and users exist) |
| POST | `/auth/login` | Password sign-in; sets the session cookie. | none |
| POST | `/auth/logout` | Delete the session and clear the cookie. | none |
| GET | `/auth/me` | Current user and workspaces with roles. | authenticated |
| GET | `/auth/oidc/login` | Start OIDC (redirect to the provider; `next` = relative return path). | none |
| GET | `/auth/oidc/callback` | OIDC redirect target; creates the session and redirects to `ORYN_PUBLIC_URL`. | none |

### workspaces

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/workspaces` | List the caller's workspaces (a token sees only its own). | authenticated |
| POST | `/workspaces` | Create a workspace (caller becomes owner). | session only (tokens refused) |
| GET | `/workspaces/{ws}` | Workspace detail. | member |
| PATCH | `/workspaces/{ws}` | Rename, toggle `require_approval`. | `workspace:manage` |
| GET | `/workspaces/{ws}/members` | List members. | `workspace:read` |
| POST | `/workspaces/{ws}/members` | Add an existing account by email with a role. | `members:manage`, and the role may not exceed the caller's |
| PATCH | `/workspaces/{ws}/members/{user_id}` | Change a member's role (at least one owner must remain). | `members:manage` |
| DELETE | `/workspaces/{ws}/members/{user_id}` | Remove a member. | `members:manage` |
| GET | `/workspaces/{ws}/teams` | List teams. | `workspace:read` |
| POST | `/workspaces/{ws}/teams` | Create a team. | `members:manage` |
| DELETE | `/workspaces/{ws}/teams/{team_id}` | Delete a team. | `members:manage` |
| GET | `/workspaces/{ws}/teams/{team_id}/members` | List a team's members. | `workspace:read` |
| PUT | `/workspaces/{ws}/teams/{team_id}/members/{user_id}` | Add a member to a team. | `members:manage` |
| DELETE | `/workspaces/{ws}/teams/{team_id}/members/{user_id}` | Remove a member from a team. | `members:manage` |
| GET | `/workspaces/{ws}/tokens` | List API tokens (all with `members:manage`, otherwise the caller's own). | member |
| POST | `/workspaces/{ws}/tokens` | Create an API token; the secret is returned once. | session only; token role ≤ caller's role |
| DELETE | `/workspaces/{ws}/tokens/{id}` | Revoke a token. | own token, or `members:manage` |
| GET | `/workspaces/{ws}/audit` | Paginated audit log, filter by `action`. | `audit:read` |
| GET | `/roles` | The role → permission matrix enforced by the server. | authenticated |

### projects

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/workspaces/{ws}/projects` | Projects visible to the caller. | member |
| POST | `/workspaces/{ws}/projects` | Create a project. | `project:write` (workspace role) |
| GET | `/projects/{id}` | Project detail. | `project:read` |
| PATCH | `/projects/{id}` | Update name, description, visibility, colour. | `project:write` |
| DELETE | `/projects/{id}` | Soft-delete a project. | `project:delete` |
| GET | `/projects/{id}/permissions` | List project grants. | `project:read` |
| POST | `/projects/{id}/permissions` | Grant a user or team a project role. | `members:manage` |
| DELETE | `/projects/{id}/permissions/{perm_id}` | Remove a grant. | `members:manage` |
| GET | `/projects/{id}/environments` | List environments (secret names only). | `project:read` |
| POST | `/projects/{id}/environments` | Create an environment. | `project:write`; `secrets:manage` when `secrets` is present |
| PUT | `/projects/{id}/environments/{env_id}` | Replace an environment. | `project:write`; `secrets:manage` when `secrets` is present |
| DELETE | `/projects/{id}/environments/{env_id}` | Delete an environment. | `project:write` |
| GET | `/projects/{id}/test-data` | List test data sets. | `project:read` |
| POST | `/projects/{id}/test-data` | Create or replace a test data set. | `test:write` |
| DELETE | `/projects/{id}/test-data/{data_id}` | Delete a test data set. | `test:write` |

### repositories

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/workspaces/{ws}/connections` | List GitHub/GitLab connections (no tokens). | `workspace:read` |
| POST | `/workspaces/{ws}/connections` | Add a connection with an encrypted token. | `secrets:manage` |
| DELETE | `/workspaces/{ws}/connections/{id}` | Delete a connection. | `secrets:manage` |
| POST | `/workspaces/{ws}/connections/{id}/verify` | Check the token against the provider's current-user endpoint. | `workspace:read` |
| GET | `/projects/{id}/repositories` | List repositories. | `project:read` |
| POST | `/projects/{id}/repositories` | Import from GitHub, GitLab, any https git URL, or a local directory under `ORYN_LOCAL_IMPORT_ROOT`; analysis runs in the background (`202`). | `project:write` |
| POST | `/projects/{id}/repositories/upload` | Upload a ZIP (raw request body), validated before extraction (`202`). | `project:write` |
| POST | `/projects/{id}/repositories/{repo_id}/analyze` | Re-import (git sources re-clone) and re-analyse; computes the change set. | `project:write` |
| DELETE | `/projects/{id}/repositories/{repo_id}` | Delete a repository and its working copy (local imports are not touched). | `project:write` |
| GET | `/projects/{id}/context` | Application context: analysis snapshot and modules. | `project:read` |
| GET | `/projects/{id}/context/search` | Full-text search over analysed (redacted) content. | `project:read` |

### requirements

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/projects/{id}/requirements` | List requirements. | `project:read` |
| POST | `/projects/{id}/requirements` | Create a requirement. | `project:write` |
| POST | `/projects/{id}/requirements/import` | Split a Markdown/text document into requirements (`##` headings, `KEY-n` prefixes become keys). | `project:write` |
| PATCH | `/projects/{id}/requirements/{rid}` | Update a requirement. | `project:write` |
| DELETE | `/projects/{id}/requirements/{rid}` | Soft-delete a requirement. | `project:write` |
| POST | `/projects/{id}/requirements/{rid}/links` | Link a scenario manually. | `test:write` |
| DELETE | `/projects/{id}/requirements/{rid}/links/{scenario_id}` | Unlink a scenario. | `test:write` |
| GET | `/projects/{id}/traceability` | Requirement → scenario → latest result matrix with coverage figures (see [METRICS.md](METRICS.md)). | `project:read` |

### testing

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/step-library` | The closed library of supported Gherkin step phrasings. | none |
| POST | `/projects/{id}/gherkin/validate` | Compile Gherkin against the step library without saving. | `project:read` |
| GET | `/projects/{id}/suites` | List suites. | `project:read` |
| POST | `/projects/{id}/suites` | Create a suite from Gherkin; scenarios start as `draft`. | `test:write` |
| GET | `/projects/{id}/suites/{suite_id}` | Suite with its scenarios. | `project:read` |
| PUT | `/projects/{id}/suites/{suite_id}` | Save edited Gherkin; scenarios matched by name keep history and approval unless their steps changed. | `test:write` |
| DELETE | `/projects/{id}/suites/{suite_id}` | Soft-delete a suite. | `test:write` |
| GET | `/projects/{id}/scenarios` | List scenarios (filters in OpenAPI). | `project:read` |
| GET | `/projects/{id}/scenarios/{scenario_id}` | Scenario detail with steps, links and history. | `project:read` |
| POST | `/projects/{id}/scenarios/status` | Approve, return to draft, or archive scenarios (1–1000 ids). Only executable scenarios can be approved. | `test:approve` to approve, otherwise `test:write` |
| GET | `/projects/{id}/step-definitions` | List custom step definitions. | `project:read` |
| POST | `/projects/{id}/step-definitions` | Create a composite step definition (body must use library steps). | `test:write` |
| DELETE | `/projects/{id}/step-definitions/{def_id}` | Delete a step definition. | `test:write` |
| GET | `/projects/{id}/recordings` | List browser recordings. | `project:read` |
| POST | `/projects/{id}/recordings` | Start a headed recording on the configured recorder (`ORYN_RECORDER_URL`). | `test:write` |
| GET | `/projects/{id}/recordings/{rec_id}` | Recording with its generated Gherkin. | `project:read` |
| POST | `/projects/{id}/recordings/{rec_id}/stop` | Stop recording and convert events to library steps (typed passwords become `{{password}}`). | `test:write` |

### planner

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/projects/{id}/plans` | List test plans. | `project:read` |
| POST | `/projects/{id}/plans` | Generate a `draft` plan; `mode` = `auto` (AI when configured), `ai` or `heuristic`. | `test:write`; plus `ai:use` when a model is used |
| GET | `/projects/{id}/plans/{plan_id}` | Plan with its items. | `project:read` |
| PATCH | `/projects/{id}/plans/{plan_id}/items/{item_id}` | Edit an item's Gherkin (recompiled) or set it `proposed`/`accepted`/`rejected`. Draft plans only. | `test:write` |
| POST | `/projects/{id}/plans/{plan_id}/approve` | Turn accepted (or all non-rejected) executable items into suites of approved scenarios, linked to requirements. | `test:approve` |
| POST | `/projects/{id}/plans/{plan_id}/reject` | Reject a draft plan. | `test:approve` |

### executions

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/workspaces/{ws}/executions` | Paginated executions in visible projects; filters `project_id`, `status`, `trigger`, `suite_id`, `order=asc`. | member |
| POST | `/projects/{id}/executions` | Queue an execution of runnable scenarios selected by suite, ids or tags (`201`, or `422` when nothing is runnable). | `execution:run` |
| GET | `/executions/{eid}` | Execution with jobs, artifacts and investigations. | `project:read` |
| POST | `/executions/{eid}/cancel` | Cancel queued jobs and request cancellation of running ones. | `execution:cancel` |
| POST | `/executions/{eid}/rerun` | New execution of the same scenarios (`failed_only` optional) with the same retry policy. | `execution:run` |
| GET | `/executions/{eid}/events` | Server-sent events (see above). | `project:read` |
| GET | `/executions/compare` | Scenario-by-scenario comparison of `base` and `head` executions of one project. | `project:read` on both |
| GET | `/jobs/{job_id}` | Job detail: steps, logs, network, artifacts, investigations, other attempts. | `project:read` |
| GET | `/artifacts/{aid}/content` | Artifact bytes (images, video, JSON, text inline; others as attachment), sandboxed. | `project:read` |

### workers

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/workspaces/{ws}/workers` | Workspace workers and shared workers. | `workspace:read` |
| POST | `/workspaces/{ws}/workers` | Pre-register a workspace worker; the `oryn_wrk_` token is returned once. | `worker:manage` |
| GET | `/workspaces/{ws}/workers/{id}` | Worker with capabilities and recent jobs (this workspace's only, for shared workers). | `workspace:read` |
| DELETE | `/workspaces/{ws}/workers/{id}` | Revoke a worker; its leased jobs are retried. Shared workers need an instance admin. | `worker:manage` |
| GET | `/workspaces/{ws}/queue` | Queued and running job counts. | `workspace:read` |

### worker-protocol

Authenticated with a worker token, not a user credential.

| Method | Path | Purpose |
|---|---|---|
| POST | `/worker/register` | Self-registration of a shared worker with `ORYN_WORKER_BOOTSTRAP_TOKEN` (`201`). |
| POST | `/worker/heartbeat` | Liveness, active jobs, draining flag, capabilities. |
| POST | `/worker/claim` | Lease the next queued job (`204` when none). Draining workers always get `204`. |
| POST | `/worker/jobs/{job_id}/start` | Mark a claimed job running. |
| POST | `/worker/jobs/{job_id}/heartbeat` | Renew the lease; `cancel_requested: true` tells the worker to stop. |
| POST | `/worker/jobs/{job_id}/artifacts` | Upload one artifact (`application/octet-stream` body; query `name`, `kind`, `content_type`). |
| POST | `/worker/jobs/{job_id}/complete` | Report the `JobResult`; returns execution status and any retry job id. |

### intelligence

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/workspaces/{ws}/overview` | Counts, pass/failure rates and durations over a window (`days`, default 30, 1–365; optional `project_id`). | member (visible projects) |
| GET | `/workspaces/{ws}/trends` | Daily execution outcome series. | member |
| GET | `/workspaces/{ws}/flaky` | Flaky scenarios with flakiness scores. | member |
| GET | `/workspaces/{ws}/failures` | Recent failures with investigation categories. | member |
| GET | `/projects/{id}/quality` | Project overview, scenario stats, requirement coverage, flaky list, open recommendations. | `project:read` |
| POST | `/projects/{id}/recommendations/refresh` | Recompute deterministic recommendations. | `project:read` |
| POST | `/projects/{id}/recommendations/{rid}/dismiss` | Dismiss a recommendation. | `test:write` |
| POST | `/jobs/{job_id}/investigate` | Re-run the deterministic investigation, optionally with an AI interpretation (`ai: true`). | `project:read`; plus `ai:use` for AI |
| GET | `/investigations/{inv_id}` | One investigation. | `project:read` |
| POST | `/projects/{id}/regression/select` | Rank runnable scenarios against changed files (rules in [METRICS.md](METRICS.md)). | `project:read` |
| POST | `/projects/{id}/regression/run` | Select and queue the selected scenarios (trigger `regression`). | `execution:run` |

### ai

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/workspaces/{ws}/ai/providers` | List providers and models (keys masked). | `workspace:read` |
| POST | `/workspaces/{ws}/ai/providers` | Add a provider (key encrypted); seeds default models for Anthropic and Mock. | `ai:configure` |
| PATCH | `/workspaces/{ws}/ai/providers/{pid}` | Update a provider (key, base URL, enabled, default). | `ai:configure` |
| DELETE | `/workspaces/{ws}/ai/providers/{pid}` | Delete a provider. | `ai:configure` |
| POST | `/workspaces/{ws}/ai/providers/{pid}/models` | Add a model with prices. | `ai:configure` |
| DELETE | `/workspaces/{ws}/ai/providers/{pid}/models/{mid}` | Delete a model. | `ai:configure` |
| POST | `/workspaces/{ws}/ai/providers/{pid}/test` | Send a tiny structured request to verify key, model and JSON support. | `ai:configure` |
| GET | `/workspaces/{ws}/ai/usage` | Requests, tokens and cost by model and purpose. | `workspace:read` |

### integrations

| Method | Path | Purpose | Permission |
|---|---|---|---|
| GET | `/workspaces/{ws}/integrations` | List integrations. | `workspace:read` |
| POST | `/workspaces/{ws}/integrations` | Create a `webhook` or `slack` integration (other kinds return `422`, not implemented). | `integration:manage` |
| PATCH | `/workspaces/{ws}/integrations/{id}` | Update URL, events, secret, enabled. | `integration:manage` |
| DELETE | `/workspaces/{ws}/integrations/{id}` | Delete. | `integration:manage` |
| POST | `/workspaces/{ws}/integrations/{id}/test` | Send an `integration.test` delivery. | `integration:manage` |
| GET | `/workspaces/{ws}/integrations/{id}/deliveries` | Delivery log. | `workspace:read` |

Webhook deliveries are `POST` with `content-type: application/json`, `x-oryn-event: <event>` and, when a secret is set, `x-oryn-signature: sha256=<hex HMAC-SHA256 of the body>`. Events: `execution.finished`, `execution.failed` (sent in addition when the execution did not pass). Payload: `{event, execution, url}`.

## Walkthrough with curl

Against a local API (`make dev`, port 8080). The cookie jar holds the session; every write sends `x-oryn-csrf: 1`. Requires `jq`.

```bash
API=http://localhost:8080/api/v1
J='content-type: application/json'
C='x-oryn-csrf: 1'

# 1. Sign in (or POST $API/auth/register with {"email","name","password"} on a fresh instance)
curl -s -c jar -H "$J" -d '{"email":"admin@example.test","password":"correct horse battery"}' \
  $API/auth/login | tee me.json
WS=$(jq -r '.workspaces[0].id' me.json)

# 2. Create a project
PID=$(curl -s -b jar -H "$J" -H "$C" -d '{"name":"Shop"}' \
  $API/workspaces/$WS/projects | jq -r .id)

# 3. Add the default environment (where tests run)
curl -s -b jar -H "$J" -H "$C" \
  -d '{"name":"local","base_url":"http://localhost:4000","api_base_url":"http://localhost:4000","is_default":true}' \
  $API/projects/$PID/environments

# 4. Create a suite from Gherkin (scenarios start as draft)
SUITE=$(jq -n --arg src 'Feature: Catalogue
  @api
  Scenario: Products are listed
    When I send a GET request to "/api/products"
    Then the response status should be 200' '{source:$src}' |
  curl -s -b jar -H "$J" -H "$C" -d @- $API/projects/$PID/suites | jq -r .suite_id)

# 5. Approve its scenarios
IDS=$(curl -s -b jar "$API/projects/$PID/suites/$SUITE" | jq '[.scenarios[].id]')
curl -s -b jar -H "$J" -H "$C" -d "{\"scenario_ids\":$IDS,\"status\":\"approved\"}" \
  $API/projects/$PID/scenarios/status

# 6. Run the suite
EID=$(curl -s -b jar -H "$J" -H "$C" -d "{\"suite_id\":\"$SUITE\"}" \
  $API/projects/$PID/executions | jq -r .id)

# 7. Poll until terminal (or stream: curl -N -b jar $API/executions/$EID/events)
until curl -s -b jar $API/executions/$EID | jq -e '.execution.status | IN("passed","failed","error","cancelled")' >/dev/null; do sleep 2; done
curl -s -b jar $API/executions/$EID | jq '.execution | {status,total,passed,failed,errored}'
```

With an API token instead of a cookie, replace `-b jar -H "$C"` with `-H "Authorization: Bearer $ORYN_TOKEN"`; the token's workspace must own the project. A worker must be running (`make dev` starts one) for the execution to leave `queued`.
