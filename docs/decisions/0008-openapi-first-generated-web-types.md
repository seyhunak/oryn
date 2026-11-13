# 0008: OpenAPI generated from handlers, with generated web types

- Status: accepted
- Date: 2026-11-13 (recorded retrospectively from the code)

## Context and problem statement

The web app, CI scripts and third-party clients all consume the REST API. Hand-maintained API documentation and hand-written client types drift from the server. How are the API contract and the web client kept in agreement?

## Decision drivers

- One source of truth for routes and schemas.
- Compile-time detection in the web app when the API changes.
- A published, browsable reference for API users.

## Considered options

1. Generate OpenAPI from Rust handler annotations (`utoipa`), check the document in, and generate TypeScript types from it.
2. Write the OpenAPI document by hand and generate Rust server stubs from it.
3. No OpenAPI; hand-written TypeScript types.

## Decision outcome

Option 1:

- Every handler has a `#[utoipa::path]` annotation and request/response types derive `ToSchema`. Routes are registered through `utoipa-axum`, so a route without an annotation does not appear in the router's document.
- The API serves the document at `/api/openapi.json` and a ReDoc page at `/api/docs`.
- `make openapi` fetches the document from a running API into [`docs/api/openapi.json`](../api/openapi.json) and runs `openapi-typescript` to produce `apps/web/lib/api/schema.d.ts`. The web app calls the API through `openapi-fetch` with those types.
- CI regenerates the types from the checked-in document and fails if they differ.

### Consequences

- Good: the served document always matches the running code (130 operations at the time of writing).
- Good: a changed response type breaks the web build instead of failing at run time.
- Bad: the checked-in document must be refreshed from a running API; CI checks that the types match the checked-in document, not that the document matches the code.
- Bad: permission requirements are not expressed in the OpenAPI document; they are documented in [API_SPEC.md](../API_SPEC.md).

## Pros and cons of the other options

- **Spec-first with generated stubs**: strong contract, but Rust server generators for axum are immature and generated code is awkward to extend.
- **Hand-written types**: no tooling, but guaranteed drift.
