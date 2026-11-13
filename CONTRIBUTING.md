# Contributing to ORYN

Thank you for helping. ORYN is an open-source, self-hosted quality-engineering platform,
and contributions of every size are welcome: bug reports, docs, tests, step-library
additions, providers, UI work.

## Ground rules

These are the project's non-negotiables. Pull requests that break them are not merged.

1. **Evidence decides.** A test passes only when the Rust execution engine verified its
   assertions. No model output may set, change or imply a verdict.
2. **No unrestricted access for models.** Models never get a shell, filesystem or network
   access. They receive fenced, redacted input and must return schema-validated output.
3. **Humans approve.** AI-planned scenarios become runnable only after a person approves
   them. ORYN never changes application code on its own.
4. **No secrets in logs or evidence.** New code paths that touch credentials must redact
   them (see `oryn_auth::redact` and the worker's secret handling).
5. **Useful without AI.** Every feature needs a deterministic path that works with no
   model provider configured.

## Getting started

```bash
make install   # Rust crates, web and runner dependencies, Chromium
make deps      # Postgres (pgvector) and Redis in Docker
make dev       # API, worker, web, runner and the demo app
make seed      # demo project with requirements and suites
```

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the full setup, ports and layout.

## Making a change

1. Open an issue first for anything larger than a small fix, so we can agree on the
   approach.
2. Work in vertical slices: database → API (with OpenAPI annotations) → UI → tests.
   Do not ship UI for a capability the backend does not have.
3. Keep the API contract in sync: after changing endpoints run `make openapi` to
   regenerate `docs/api/openapi.json` and the web client types. CI fails when they drift.
4. Follow the design system in [design.md](design.md) for UI work.
5. Update docs that describe what you changed, including
   [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md). A feature is marked
   "Verified" only when a test or a recorded run demonstrates it.

## Checks

```bash
make lint   # rustfmt, clippy -D warnings, eslint + tsc
make test   # Rust (needs `make deps`), web, runner, demo app
```

Tests never call real model providers; use the mock provider or a local mock server.

## Commit and pull request style

- Conventional commit subjects (`feat(api): …`, `fix(worker): …`, `docs: …`).
- One logical change per pull request, with a description of what and why, how you
  tested it, and screenshots for UI changes.
- New dependencies need a reason in the description.

## Licence

By contributing you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE).

Please read our [code of conduct](CODE_OF_CONDUCT.md).
