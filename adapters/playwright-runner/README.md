# ORYN Playwright runner

An isolated browser-execution adapter for ORYN. The Rust worker sends it an ordered
list of primitive steps (`goto`, `click`, `fill`, `observe_text`, …). The runner
executes them in a real browser through [Playwright](https://playwright.dev) and
returns raw results, *observations*, screenshots, video, traces, console messages
and network failures.

It contains **no scheduling or business logic** and never decides pass/fail.
Observation steps report what they saw (`{"visible": false}`) instead of failing; the
Rust engine turns observations into verdicts.

The wire contract is **[`docs/protocol/browser-v1.md`](../../docs/protocol/browser-v1.md)**.

## Quick start

```bash
cd adapters/playwright-runner
npm install
npx playwright install chromium          # add firefox webkit if you need them
npm run build && npm start               # http://0.0.0.0:7400
# or, for development with reload:
npm run dev
```

```bash
curl -s localhost:7400/v1/health

curl -s -X POST localhost:7400/v1/runs -H 'content-type: application/json' -d '{
  "protocol": "oryn.browser/v1",
  "run_id": "demo-1",
  "base_url": "http://localhost:4000",
  "allowed_hosts": ["localhost"],
  "steps": [
    {"id": "s1", "action": "goto", "url": "/login"},
    {"id": "s2", "action": "fill", "target": "Email", "value": "demo@northwind.test"},
    {"id": "s3", "action": "fill", "target": "Password", "value": "demo-password"},
    {"id": "s4", "action": "click", "target": "Sign in"},
    {"id": "s5", "action": "observe_text", "text": "Welcome, Demo Customer"}
  ]
}'
```

(The base URL above is the [Northwind Supply demo app](../../examples/demo-application).)

## Configuration

| Env | Default | Meaning |
| --- | --- | --- |
| `PORT` | `7400` | Listen port |
| `HOST` | `0.0.0.0` | Bind address |
| `RUNNER_TOKEN` | unset | If set, every route except `/v1/health` requires `Authorization: Bearer <token>` |
| `MAX_CONCURRENCY` | `2` | Runs executed in parallel; extra runs queue (FIFO) |
| `ORYN_RUN_TIMEOUT_MS` | `300000` | Upper bound for one run; when exceeded the run is `aborted` |
| `ORYN_RECORDER_HEADLESS` | unset | `1` runs the recorder headless (for tests/CI only) |

## Endpoints

| Route | Purpose |
| --- | --- |
| `GET /v1/health` | Protocol version and per-browser availability (public) |
| `POST /v1/runs` | Execute a run synchronously and return the results |
| `POST /v1/recordings` | Start a headed recording session (`{url, browser?}`) |
| `GET /v1/recordings/:id` | Recorded events so far |
| `POST /v1/recordings/:id/stop` | Close the browser and return the final events |

## Behaviour in brief

- **Isolation:** one new browser context per run. One shared browser process per
  engine, launched lazily and relaunched if it crashes.
- **Targets:** `css=`, `xpath=`, `text=` or raw CSS (`#id`, `.class`, `[attr]`, `>>`).
  Plain text tries the button role, link role, label, placeholder, test id and text,
  in that order. Form actions (`fill`, `select`, `check`, `uncheck`) try label and
  placeholder first.
- **Failures:** an action error records `{kind, message}`, captures `<id>-failure.png`
  (unless `screenshots: "off"`) and skips the remaining steps.
- **Network policy:** a non-empty `allowed_hosts` blocks every other host.
  `169.254.169.254` is always blocked. Blocked requests are reported as
  `failure: "blocked by policy"`.
- **Artifacts:** returned inline as base64. Each is capped at 25 MiB; anything larger
  is dropped, with a console note.
- **Recorder redaction:** password fields, and fields whose name, id, autocomplete or
  label matches `/pass|secret|token|card|cvv|ssn|otp/i`, are recorded as
  `"{{secret}}"` with `"redacted": true`. The raw value never leaves the page.

## Development

```bash
npm run typecheck   # tsc --noEmit (strict)
npm run build       # tsc -> dist/
npm test            # node:test via tsx: unit tests + Chromium integration tests
```

The integration tests start a tiny local static site and drive real headless
Chromium. If Chromium is not installed they are **skipped**, not failed.

Source layout:

| File | Responsibility |
| --- | --- |
| `src/server.ts` | HTTP routing, auth, error mapping, startup |
| `src/protocol.ts` | zod schemas and wire types |
| `src/executor.ts` | Run execution, observations, artifacts, console and network capture |
| `src/targets.ts` | Target to strategy resolution (pure) and polling |
| `src/policy.ts` | Network allow-list decision (pure) |
| `src/browsers.ts` | Browser pool and availability probe |
| `src/recorder.ts` | Session recorder and injected capture script |
| `src/redaction.ts` | Sensitive field predicate (pure) |
| `src/semaphore.ts` | FIFO concurrency limiter |

## Docker

```bash
docker build -t oryn-playwright-runner adapters/playwright-runner
docker run --rm -p 7400:7400 --ipc=host -e RUNNER_TOKEN=change-me oryn-playwright-runner
```

The image is based on `mcr.microsoft.com/playwright:v1.65.0-noble`, which ships all
three browsers. The tag must match the `playwright` version in `package.json`. The
service runs as the non-root `pwuser`. Chromium needs shared memory, so use
`--ipc=host` or `--shm-size=1g`. The container has no display, so
`POST /v1/recordings` returns `501 no_display` there. Run the recorder on a desktop
machine instead.

To reach an app on the Docker host from inside the container, use
`host.docker.internal` as `base_url` and include it in `allowed_hosts`.

## License

Apache-2.0
