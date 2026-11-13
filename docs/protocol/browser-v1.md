# `oryn.browser/v1`: browser execution protocol

Status: **v1, implemented** by `adapters/playwright-runner` (TypeScript + Playwright).
This document is the contract between the ORYN Rust worker (client) and a browser
runner (server). If the code and this document disagree, that is a bug in one of them.

## Design principles

1. **The runner has no business logic.** It doesn't schedule, retry, or decide pass/fail.
   The worker sends an ordered list of primitive steps. The runner executes them and
   returns raw results.
2. **Observations never fail.** `observe_*` steps report what they saw (for example
   `{"visible": false}`). The Rust engine turns observations into assertions. An
   observation step has `status: "error"` only when something actually crashed,
   such as the page or browser going away.
3. **Every run is isolated.** Each run gets a new browser context: fresh cookies,
   storage and cache. Browser *processes* are shared and reused across runs.
4. **Artifacts are returned inline** (base64) so the worker needs no shared filesystem.

## Transport

- HTTP/1.1, JSON bodies (`content-type: application/json`), UTF-8.
- Default listen address `0.0.0.0:7400` (env `HOST`, `PORT`).
- Request bodies are limited to 5 MiB (`413 payload_too_large`).
- `POST /v1/runs` holds the connection open until the run finishes. Client timeouts
  should exceed `ORYN_RUN_TIMEOUT_MS` (default 300 s) plus queueing time.

### Authentication

If the runner has `RUNNER_TOKEN` set, every route **except** `GET /v1/health` requires
`Authorization: Bearer <RUNNER_TOKEN>`. Missing or invalid tokens get
`401 {"error":{"code":"unauthorized",...}}`. Without `RUNNER_TOKEN`, auth is off.

### Errors

Errors outside a run (as opposed to step errors inside a run response) always use
this shape:

```json
{ "error": { "code": "invalid_request", "message": "steps.0.target: Invalid input: expected string, received undefined" } }
```

| HTTP | `code` | When |
| --- | --- | --- |
| 400 | `invalid_request` | Body is not JSON, fails schema validation, has a duplicate step id, or `protocol` is missing |
| 400 | `unsupported_protocol` | `protocol` is present but not `"oryn.browser/v1"` |
| 401 | `unauthorized` | Bearer token missing or wrong |
| 404 | `not_found` | Unknown route or recording id |
| 405 | `method_not_allowed` | Wrong HTTP method |
| 413 | `payload_too_large` | Body over 5 MiB |
| 501 | `no_display` | Recorder could not launch a headed browser |
| 503 | `browser_unavailable` | The requested browser is not installed or failed to launch |
| 500 | `internal_error` | Unexpected failure |

## `GET /v1/health`

Always public. Each browser is probed once, on first call or at startup: it is launched
headless and the result is cached. A missing browser never crashes the runner.

```json
{
  "status": "ok",
  "protocol": "oryn.browser/v1",
  "browsers": [
    { "name": "chromium", "version": "158.0.8127.18", "available": true },
    { "name": "firefox",  "version": null,            "available": false },
    { "name": "webkit",   "version": "27.2",          "available": true }
  ]
}
```

`version` is `browser.version()`, or `null` when the browser is unavailable. The
`browsers` array always lists `chromium`, `firefox`, `webkit` in that order.

## `POST /v1/runs`

### Request

```json
{
  "protocol": "oryn.browser/v1",
  "run_id": "3f6c1a2e-8b1d-4c1e-9f7a-0e2d5b6c7a01",
  "browser": "chromium",
  "base_url": "http://localhost:4000",
  "viewport": { "width": 1280, "height": 800 },
  "step_timeout_ms": 15000,
  "screenshots": "on_failure",
  "record_video": false,
  "record_trace": false,
  "headers": { "X-Foo": "bar" },
  "storage_state": null,
  "allowed_hosts": ["localhost", "127.0.0.1"],
  "steps": [ { "id": "s1", "action": "goto", "url": "/login" } ]
}
```

| Field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `protocol` | string | yes | | Must be `"oryn.browser/v1"` |
| `run_id` | string (1–128) | yes | | Echoed back; a UUID by convention |
| `browser` | `"chromium" \| "firefox" \| "webkit"` | no | `"chromium"` | |
| `base_url` | http(s) URL | yes | | Relative `goto` URLs resolve against it |
| `viewport` | `{width: 200–7680, height: 200–4320}` | no | `{1280, 800}` | Also the video size |
| `step_timeout_ms` | int 100–120000 | no | `15000` | Per-step budget, also the default observation wait |
| `screenshots` | `"off" \| "on_failure" \| "every_step"` | no | `"on_failure"` | `screenshot` steps always capture |
| `record_video` | bool | no | `false` | WebM of the whole run |
| `record_trace` | bool | no | `false` | Playwright trace zip (screenshots + snapshots) |
| `headers` | `{string: string}` | no | `{}` | Extra HTTP headers on every request |
| `storage_state` | Playwright storage state or `null` | no | `null` | `{cookies:[{name,value,domain,path,expires,httpOnly,secure,sameSite}], origins:[{origin, localStorage:[{name,value}]}]}` |
| `allowed_hosts` | string[] (≤256) | no | `[]` | Network allow-list; see [Network policy](#network-policy) |
| `steps` | Step[] (1–500) | yes | | Step ids must be unique within the run |

Unknown fields are rejected (`invalid_request`), both at the top level and inside steps.

### Steps

Every step has an `id` (string, 1–128 characters, unique in the run) and an `action`.

#### Action steps

| `action` | Fields | Behaviour |
| --- | --- | --- |
| `goto` | `url` | `new URL(url, base_url)`, then navigate and wait for `load`. If the URL fails the network policy, the step errors with kind `blocked` and no request is made. An HTTP error status is **not** a step error; it shows up in `network`. |
| `click` | `target` | Resolve the target, click, then wait (best effort, within the step budget) for any `load` the click triggered |
| `fill` | `target`, `value` | Resolve the target (form intent), clear it, type the value |
| `press` | `key`, `target?` | With a target: focus it and press. Without: press on the focused element. Then wait for `load`, as for `click`. Keys use Playwright names (`Enter`, `Tab`, `Control+A`). |
| `select` | `target`, `value` | Resolve the target (form intent). Select the option whose value **or** label equals `value`. |
| `check` / `uncheck` | `target` | Resolve the target (form intent), then set the checked state |
| `hover` | `target` | |
| `wait` | `ms` (int ≥ 0) | Sleep `min(ms, 30000)` ms |
| `screenshot` | `name` | Viewport PNG named `<sanitised name>.png` (characters outside `[A-Za-z0-9._-]` become `_`). The step's `screenshot` field holds the artifact name. |

#### Observation steps

Observations accept optional **wait hints**. The runner polls about every 100 ms until
the hint holds or `wait_ms` runs out, then returns the observation as it stands.
`wait_ms` defaults to `step_timeout_ms` and is capped by it, and by the time left in
the run. An observation with no hint returns immediately.

| `action` | Fields | Observation | Waits until |
| --- | --- | --- | --- |
| `observe_text` | `text`, `visible?` (default `true`), `wait_ms?` | `{ "visible": bool, "count": int }` | the text's visibility equals `visible` |
| `observe_element` | `target`, `contains?`, `wait_ms?` | `{ "visible": bool, "count": int, "text": string\|null, "value": string\|null }` | the element is visible, and its `innerText` or value contains `contains` (if given) |
| `observe_url` | `contains?`, `wait_ms?` | `{ "url": "<full url>", "path": "/x" }` | `page.url()` contains `contains` (no hint: immediate) |
| `observe_title` | `equals?`, `wait_ms?` | `{ "title": string }` | the title equals `equals` (no hint: immediate) |

Semantics:

- `observe_text` matches like Playwright `getByText(text)`: a case-insensitive
  substring, with whitespace normalised. `count` is the number of **visible**
  matching elements. A timeout is **not** an error: it returns
  `{"visible": false, "count": 0}`. For "should not see", send `visible: false`; the
  observation returns as soon as the text is gone, or after `wait_ms` with
  `visible: true`. Use `wait_ms: 0` for an instantaneous check.
- `observe_element` resolves `target` with the [target rules](#target-resolution)
  (default order). It reports the first strategy that matches. `count` is that
  strategy's match count. `text` is the trimmed `innerText` of the first match (`""`
  for inputs). `value` is the current value for `input`, `textarea` and `select`, and
  `null` otherwise. If nothing matches, the result is
  `{"visible": false, "count": 0, "text": null, "value": null}`.
- `observe_url.path` is the URL's `pathname`, without the query string.

### Target resolution

A `target` string is turned into an ordered list of strategies:

| Target | Strategy |
| --- | --- |
| `css=<selector>` | CSS selector |
| `xpath=<expr>` | XPath |
| `text=<text>` | Exact text: `getByText(text, {exact: true})` |
| starts with `#`, `.` or `[`, or contains `>>` | CSS selector (as given) |
| anything else | Try in order, use the first with ≥ 1 match (see below) |

For plain text, the **default order** applies to `click`, `hover`, `press` and
`observe_element`:

1. `getByRole('button', {name, exact: true})`
2. `getByRole('link', {name, exact: true})`
3. `getByRole('button', {name, exact: false})`
4. `getByRole('link', {name, exact: false})`
5. `getByLabel(target)`
6. `getByPlaceholder(target)`
7. `getByTestId(target)`
8. `getByText(target)`

Exact names come first so that a target naming a link ("Cordless Drill 18V") is not
captured by a longer button that contains it ("Add Cordless Drill 18V to cart").

The **form order** applies to `fill`, `select`, `check` and `uncheck`, which need a
form control. Label and placeholder go first:
`getByLabel` → `getByPlaceholder` → `getByTestId` → button → link → `getByText`.
Without this, `fill "Password"` on a typical login page would resolve to the link
"Forgot your password?".

The winning locator is narrowed with `.first()`. Resolution polls about every 100 ms
until a strategy matches. If none matches within the step budget, the action step
errors with kind `not_found`.

### Execution

- A new browser context is created per run: headless, with `viewport`, `headers`,
  `storage_state`, optional video recording, and service workers blocked so every
  request passes through the network policy.
- Steps run **sequentially**. When an **action** step throws:
  - the step gets `status: "error"` and an `error` of `{kind, message}`;
  - if `screenshots != "off"`, a PNG named `<step id>-failure.png` is captured and
    referenced from the step;
  - all remaining steps are left as `status: "skipped"`, with `started_at: null`
    and `duration_ms: 0`.
- Observation steps follow the same rule when they crash, which is rare.
- With `screenshots: "every_step"`, a PNG `<step id>.png` is captured after every
  successful step, except `screenshot` steps, which already capture one.
- **Run timeout:** the whole run is bounded by the runner's `ORYN_RUN_TIMEOUT_MS`
  (default 300000). When it expires, the step in progress gets
  `error.kind = "timeout"`, the remaining steps are skipped, and the run `status` is
  `"aborted"`. Each step's budget is `min(step_timeout_ms, time left in the run)`.
- The context is always closed. Video files are only complete after the close.
- **Concurrency:** at most `MAX_CONCURRENCY` (default 2) runs execute at once. Extra
  requests wait in a FIFO queue; their HTTP request stays open while they wait.

Error kinds:

| `kind` | Meaning |
| --- | --- |
| `not_found` | No target strategy matched within the step budget |
| `timeout` | A Playwright timeout (for example the element never became actionable), or the run timeout |
| `navigation` | `goto` failed (DNS, connection refused, aborted), or another net error |
| `blocked` | The navigation target or request was refused by the network policy |
| `unknown` | Anything else (for example filling a non-input element) |

### Network policy

Every request in the context passes through a route handler:

- `data:` and `blob:` URLs are always allowed.
- `169.254.169.254` (the cloud metadata endpoint) is **always** blocked.
- If `allowed_hosts` is non-empty, any request whose `hostname` is not in the list is
  aborted with `blockedbyclient`. Matching is exact and case-insensitive. Ports are
  ignored. IPv6 literals may be given with or without brackets.
- Each blocked request is reported in `network` with `status: null` and
  `failure: "blocked by policy"`.

### Response (`200`)

```json
{
  "protocol": "oryn.browser/v1",
  "run_id": "3f6c1a2e-8b1d-4c1e-9f7a-0e2d5b6c7a01",
  "status": "completed",
  "browser": "chromium",
  "browser_version": "158.0.8127.18",
  "started_at": "2026-11-13T13:52:10.010Z",
  "duration_ms": 6318,
  "steps": [
    { "id": "s1", "action": "goto", "status": "ok", "started_at": "2026-11-13T13:52:10.040Z", "duration_ms": 16,
      "observation": null, "error": null, "screenshot": null },
    { "id": "s7", "action": "observe_text", "status": "ok", "started_at": "…", "duration_ms": 9,
      "observation": { "visible": true, "count": 1 }, "error": null, "screenshot": null },
    { "id": "s14", "action": "click", "status": "error", "started_at": "…", "duration_ms": 5008,
      "observation": null,
      "error": { "kind": "not_found", "message": "no element matched target \"Nonexistent button\" within 5000 ms" },
      "screenshot": "s14-failure.png" },
    { "id": "s15", "action": "observe_title", "status": "skipped", "started_at": null, "duration_ms": 0,
      "observation": null, "error": null, "screenshot": null }
  ],
  "artifacts": [
    { "name": "s14-failure.png", "kind": "screenshot", "content_type": "image/png",
      "data_base64": "iVBORw0KGgo…", "size_bytes": 32971 }
  ],
  "console": [ { "type": "log", "text": "welcome page loaded", "timestamp": "…" } ],
  "network": [
    { "url": "http://localhost:4000/missing.json", "method": "GET", "status": 404, "failure": null },
    { "url": "http://169.254.169.254/latest/meta-data", "method": "GET", "status": null, "failure": "blocked by policy" }
  ]
}
```

| Field | Notes |
| --- | --- |
| `status` | `"completed"`: every step either ran or was skipped after an error. `"aborted"`: the run timeout fired. A step error does **not** make the run `aborted`. |
| `steps` | Same order and length as the request. `status` is `ok`, `error` or `skipped`. `observation` is non-null only for an `ok` observation step. |
| `artifacts` | `kind` is `screenshot` (`image/png`), `video` (`video/webm`, name `video.webm`) or `trace` (`application/zip`, name `trace.zip`). Artifact names are unique within a run; on a collision the step id is prefixed. Any artifact over **25 MiB** is dropped, with a `warning` console entry noting it. |
| `console` | Browser console messages (`type` is Playwright's `ConsoleMessage.type()`: `log`, `error`, `warning`, …). Uncaught page exceptions appear as `type: "pageerror"`. Runner notes use `type: "warning"` and start with `[oryn]`. Capped at 1000 entries. |
| `network` | Failed requests (`status: null`, `failure` = the browser's error text, or `"blocked by policy"`) and responses with status ≥ 400 (`failure: null`). Capped at 1000 entries. |

## Session recorder

The recorder opens a **headed** browser on the runner host for a person to click
through. It needs a display, so it is not available in the stock Docker image.
`ORYN_RECORDER_HEADLESS=1` launches it headless; that mode is only for tests.

### `POST /v1/recordings`

Request: `{"url": "http://localhost:4000/login", "browser": "chromium"}` (`browser` is
optional and defaults to `chromium`).
Response: `201 {"recording_id": "<uuid>"}`. If the browser cannot launch:
`501 {"error":{"code":"no_display",...}}`.

### `GET /v1/recordings/:id` and `POST /v1/recordings/:id/stop`

Both return:

```json
{
  "recording_id": "8c1d…",
  "status": "recording",
  "url": "http://localhost:4000/login",
  "events": [
    { "id": "r1", "action": "goto", "url": "/login" },
    { "id": "r2", "action": "fill", "target": "Email", "value": "demo@northwind.test" },
    { "id": "r3", "action": "fill", "target": "Password", "value": "{{secret}}", "redacted": true },
    { "id": "r4", "action": "click", "target": "Sign in" }
  ]
}
```

`stop` closes the browser and returns `status: "stopped"`. A recording also stops if
the person closes the browser, or after 30 minutes without interaction.

`events` uses the step format above (ids `r1`, `r2`, …), so it can be replayed as a
run after review. Captured interactions:

| Interaction | Step |
| --- | --- |
| Initial page, and navigations not caused by a recorded interaction within 1.5 s | `goto` (path-relative for the same origin, absolute otherwise) |
| Click on a button, link, `[role=button]`, etc. | `click` |
| Click on a checkbox or radio | `check` / `uncheck` |
| Text input `change` (also flushed before the next interaction or Enter) | `fill` |
| `<select>` change | `select` |
| Enter key | `press` `{key: "Enter", target?}` |

A target is the most human-readable description available, in this order: `aria-label`,
label text for form fields, button or link text, `placeholder`, `data-testid`, and
finally a CSS path written as `css=…`.

**Redaction.** A `fill` is redacted when the input has `type=password`, or when its
`name`, `id`, `autocomplete` or label matches `/pass|secret|token|card|cvv|ssn|otp/i`.
Its `value` becomes `"{{secret}}"` and the event gets `"redacted": true`. Redaction
happens **inside the page**, so the raw value never crosses into the runner process.
The runner checks again on its side. Field values are never logged.

## Configuration (runner)

| Env | Default | Meaning |
| --- | --- | --- |
| `PORT` | `7400` | Listen port |
| `HOST` | `0.0.0.0` | Bind address |
| `RUNNER_TOKEN` | unset | Enables bearer auth |
| `MAX_CONCURRENCY` | `2` | Concurrent runs; extras queue |
| `ORYN_RUN_TIMEOUT_MS` | `300000` | Upper bound for one run |
| `ORYN_RECORDER_HEADLESS` | unset | `1` = recorder runs headless (tests only) |

## Versioning

Breaking changes get a new protocol string (`oryn.browser/v2`). Additive changes,
such as new optional fields, new step actions or new error kinds, may land in v1.
Clients must ignore unknown response fields.
