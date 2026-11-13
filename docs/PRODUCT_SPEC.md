# Product specification

## Purpose

ORYN is a self-hosted platform for building, running and governing automated acceptance tests. It turns requirements and source code into executable Gherkin scenarios, runs them on browser and API workers that the operator hosts, records evidence for every step, and explains failures from that evidence.

The product rests on four rules, summarised as "AI reasons. Rust executes. Evidence proves. Humans govern.":

1. **A verdict comes only from deterministic assertions.** A scenario passes or fails because the execution engine checked an expected value against an actual one. No model output can mark a test passed or failed.
2. **Evidence backs every verdict.** Each step stores its expected and actual values, logs, network records and, for browser steps, screenshots; video and traces are optional.
3. **People approve what runs.** Scenarios from the planner, recorder or editor are drafts until someone with `test:approve` approves them (unless the workspace turns `require_approval` off, in which case drafts may run but are never auto-approved).
4. **AI is optional and advisory.** Every feature works without a model provider. When one is configured, it proposes test plans and interprets failures; its output is labelled and stored separately from deterministic findings. Models have no shell, file system or network access, and ORYN never changes application code.

## Users and roles

| Role | Typical person | Can |
|---|---|---|
| `owner` | Workspace owner | Everything, including workspace settings and `require_approval`. |
| `admin` | QA lead, platform engineer | Manage members, tokens, workers, AI providers, integrations, secrets; delete projects; read the audit log. |
| `engineer` | Test or software engineer | Create and edit projects, requirements, suites; approve scenarios and plans; run and cancel executions; use AI. |
| `viewer` | Stakeholder, auditor | Read projects, results, evidence and quality data. |

Project-level grants can raise a member's role on one project, and private projects are visible only to explicit grantees and workspace admins. CI systems use API tokens with a role ceiling. The full matrix is in [API_SPEC.md](API_SPEC.md#authorisation).

## Functional areas

The web app has nine sections. Each maps to a set of API endpoints listed in [API_SPEC.md](API_SPEC.md).

| Area | Web route | What it does |
|---|---|---|
| Overview | `/overview` | Workspace figures over a window: executions, pass rate, failures, flaky scenarios, recent runs. |
| Projects | `/projects`, `/projects/[id]` | Projects with environments (base URLs, allowed hosts, variables, encrypted secrets), test data, repositories, requirements and traceability. Repository import from GitHub, GitLab, any https git URL, a ZIP upload, or a local directory; analysis detects languages, frameworks, routes, tests and modules and records what changed since the previous analysis. |
| Test Studio | `/studio`, `/studio/new`, `/studio/[suiteId]`, `/studio/scenarios/[scenarioId]` | Gherkin editor validated live against the closed step library; suites and scenarios with status (`draft`, `approved`, `archived`); scenario history; browser recording into Gherkin. |
| AI Test Planner | `/planner`, `/planner/[planId]` | Generate a draft plan from requirements and repository context, heuristically or with a model; edit, accept or reject each proposed scenario; approve the plan into suites. |
| Executions | `/executions`, `/executions/[eid]`, `/executions/[eid]/jobs/[jobId]`, `/executions/compare` | Run suites, scenarios or tags against an environment; live progress; per-step evidence; investigations; cancel, re-run (all or failed only), compare two runs. |
| Quality Intelligence | `/quality` | Trends, flaky scenarios, failure categories, requirement coverage, deterministic recommendations, change-based regression selection. |
| Workers | `/workers`, `/workers/[id]` | Worker fleet: status, capabilities, concurrency, recent jobs, queue depth; create and revoke workers. |
| Integrations | `/integrations` | Outgoing webhooks and Slack notifications with a delivery log. |
| Settings | `/settings` | Workspace, members, teams, API tokens, AI models, repository connections, audit log. |

## Core flows

### Write and run a scenario

1. Create a project and a default environment with a base URL.
2. In Test Studio, write a feature. The editor compiles it against the step library and reports unknown steps with suggestions.
3. Save it as a suite. Its scenarios start as `draft`.
4. Approve the scenarios.
5. Run the suite. ORYN creates one job per scenario; workers claim them and report step results and artifacts.
6. Watch progress live. When a job fails, ORYN classifies the failure deterministically and shows the evidence.

### Plan from requirements

1. Import requirements from a Markdown document (or add them one by one) and import the repository.
2. Generate a plan. Without a model, the heuristic planner proposes API scenarios for `GET` endpoints mentioned in acceptance criteria, browser scenarios for page paths and quoted UI text, and smoke scenarios for routes no requirement mentions. With a model, the planner sends redacted, fenced context and accepts only schema-valid output, with each item compiled against the step library.
3. Review each item: edit its Gherkin, accept or reject it.
4. Approve the plan. Accepted executable items become suites of approved scenarios linked to their requirements.

### Investigate a failure

1. Open a failed job. Steps show expected versus actual values, logs, network records and screenshots.
2. The deterministic investigation names a category (`flaky_test`, `environment_failure`, `authentication_failure`, `dependency_failure`, `test_defect`, `application_defect` or `unknown`) with a rule identifier, confidence and the observations it used.
3. Optionally request an AI interpretation. It is stored next to the deterministic one and labelled "AI interpretation"; it can fill in the category only when the rules found none.

### Run what a change affects

1. Supply changed files (or re-analyse the repository to compute them).
2. Regression selection scores runnable scenarios by route, keyword, recent failure and flakiness, always including smoke scenarios, and explains every score.
3. Run the selection; the execution is recorded with trigger `regression`.

### Automate from CI

1. Create an API token with the `engineer` role.
2. `POST /projects/{id}/executions` with `trigger: "ci"`, `commit_sha` and `branch`.
3. Poll the execution or receive an `execution.finished` webhook.

## Supported test types

- **API tests** are executed in Rust by `oryn-api-engine`: HTTP requests with headers and bodies, status, JSON-path and JSON Schema assertions, response-time limits, capturing response values into variables.
- **Browser tests** are executed by the Playwright runner in Chromium, Firefox or WebKit over the `oryn.browser/v1` protocol ([docs/protocol/browser-v1.md](protocol/browser-v1.md)).
- **Mixed scenarios** contain both kinds of steps and run in one job.

The step library is closed: a step either matches a library phrasing or the scenario cannot run. `GET /api/v1/step-library` lists every phrasing.

## Non-goals

- ORYN does not write or change application code, open pull requests or apply fixes.
- It does not give models tools, shell access or file-system access.
- It does not offer free-form test code (Playwright or other scripts) as a scenario format; scenarios are Gherkin over the step library.
- It is not a hosted service; there is no multi-instance federation or billing.
- It does not do load testing, visual diffing or mobile-device testing.

## Not implemented

The following appear in the schema or UI vocabulary but do not work yet. See [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) and [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md).

- Scheduled executions (the `schedule` trigger is accepted, but nothing schedules runs).
- Jira, GitHub, GitLab, GitHub Actions and GitLab CI integrations (only webhooks and Slack deliver).
- Object storage for artifacts (local file system only).
- Vector search over code (pgvector is installed; search uses PostgreSQL full-text search).
- AI-generated recommendations (recommendations are deterministic).
- Routing jobs to workers by label.
- Composite custom step definitions (stored and validated but not expanded at run time).
- A metrics endpoint for Prometheus.
