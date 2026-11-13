# 0003: Browser automation in an isolated Node Playwright runner over HTTP

- Status: accepted
- Date: 2026-11-13 (recorded retrospectively from the code)

## Context and problem statement

Browser steps need a real browser. Playwright is the most capable automation library and its primary implementation is Node. Browsers are heavy, crash-prone and execute untrusted page content. Where should browser automation run, and how should the Rust worker talk to it?

## Decision drivers

- Use Playwright's first-class Node API rather than a partial port.
- Keep browsers out of the API process and out of the worker's address space.
- Scale browsers separately from workers.
- Keep the step vocabulary closed: the runner executes compiled steps, not scripts.

## Considered options

1. A separate Node service (`adapters/playwright-runner`) that executes step lists over an HTTP protocol (`oryn.browser/v1`).
2. Drive browsers from Rust through the Chrome DevTools Protocol or WebDriver.
3. Generate Playwright test files and run them with the Playwright test runner.
4. Embed Node in the worker process.

## Decision outcome

Option 1. The worker sends a run request (steps, base URL, allowed hosts, browser, artifact options) to `POST /v1/runs` with a bearer `RUNNER_TOKEN`; the runner returns per-step results and base64 artifacts, which the worker uploads to the API. The protocol is specified in [docs/protocol/browser-v1.md](../protocol/browser-v1.md). The runner enforces its own network policy, redacts its output, limits concurrency (`MAX_CONCURRENCY`) and run time (`ORYN_RUN_TIMEOUT_MS`), and runs as a non-root user in the official Playwright image. It also serves browser recording.

### Consequences

- Good: Chromium, Firefox and WebKit are supported through Playwright; target resolution (role, label, placeholder, test id, text) uses Playwright's locators.
- Good: a browser crash cannot take down the worker; runners scale independently.
- Bad: protocol types and network policy are implemented twice (Rust and TypeScript). They have already diverged: the runner does not support `*.domain` wildcards and blocks fewer metadata addresses (see [SECURITY.md](../SECURITY.md#known-limitations)).
- Bad: artifacts travel as base64 in JSON, capped at 25 MB per artifact.

## Pros and cons of the other options

- **CDP/WebDriver from Rust**: one language, but no mature equivalent of Playwright's auto-waiting and locators, and Chromium-centric.
- **Generated Playwright tests**: full Playwright features, but turns scenarios back into code and makes per-step evidence harder to capture in a uniform shape.
- **Embedded Node**: avoids a network hop but couples browser failures to the worker and complicates the build.
