# 0001: Rust for the control plane and execution engine

- Status: accepted
- Date: 2026-11-13 (recorded retrospectively from the code)

## Context and problem statement

ORYN's verdicts must be trustworthy: a test passes only because an assertion was checked. The component that checks assertions, schedules jobs, stores evidence and enforces access control is the part of the system that must be correct, long-running and safe with untrusted input (ZIP files, git repositories, HTTP responses, model output). Which language should the control plane and the execution engine use?

## Decision drivers

- Correctness of the job state machine and of concurrent queue operations.
- Memory safety when parsing untrusted archives, HTTP bodies and JSON.
- Predictable resource use for a self-hosted service.
- A single static binary per component for simple deployment.

## Considered options

1. Rust for the API, worker, API engine and Gherkin compiler.
2. TypeScript/Node for everything, sharing code with the web app and the Playwright runner.
3. Go.
4. Python.

## Decision outcome

Option 1. The workspace has 12 crates (see [ARCHITECTURE.md](../ARCHITECTURE.md#rust-crates)): `axum` for HTTP, `sqlx` for PostgreSQL, `tokio` for concurrency, `utoipa` for OpenAPI. Domain enums (`ExecutionStatus`, `JobStatus`, `Role`, `Permission`) are exhaustive Rust types, so a missing case in a state transition or permission check is a compile error. The API engine, which evaluates HTTP assertions, runs in Rust inside the worker. Browser automation is the exception; see [0003](0003-isolated-playwright-runner.md).

### Consequences

- Good: lifecycle transitions are checked by the type system and unit-tested in `oryn-execution`; ZIP and git handling run in memory-safe code with explicit limits.
- Good: the API and worker ship as small Debian-based images with one binary each.
- Bad: two languages on the backend (Rust plus the Node runner), so browser protocol types are defined twice (Rust and TypeScript) and must be kept in step by hand.
- Bad: slower compile times and a smaller contributor pool than TypeScript.

## Pros and cons of the other options

- **Node everywhere**: one language and direct Playwright access, but weaker guarantees for the queue and state machine and a larger runtime surface for untrusted parsing.
- **Go**: simple concurrency and fast builds, but less expressive types for exhaustive state and permission modelling.
- **Python**: rich AI tooling, but slower and dynamically typed in the component that has to be most predictable.
