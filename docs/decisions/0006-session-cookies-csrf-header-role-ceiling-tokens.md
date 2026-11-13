# 0006: Session cookies with a CSRF header, and API tokens with a role ceiling

- Status: accepted
- Date: 2026-11-13 (recorded retrospectively from the code)

## Context and problem statement

ORYN has browser users and automated clients (CI, scripts). Browser credentials must not be readable by JavaScript or usable cross-site. Automation credentials must be revocable, scoped and unable to exceed the access of the person who created them. How do clients authenticate?

## Decision drivers

- Resistance to XSS token theft and CSRF.
- Instant revocation.
- Least privilege for automation.
- Same authorisation code for both client types.

## Considered options

1. Opaque server-side sessions in an `HttpOnly` cookie plus a required custom header on writes; opaque, hashed personal access tokens bound to a workspace with a role ceiling.
2. JWTs in local storage for the browser and for automation.
3. Cookies with a synchronizer (double-submit) CSRF token.

## Decision outcome

Option 1, in [`crates/oryn-api/src/auth.rs`](../../crates/oryn-api/src/auth.rs):

- `oryn_session` cookie: random token, SHA-256 stored, `HttpOnly`, `SameSite=Lax`, `Secure` when configured, TTL `ORYN_SESSION_TTL_HOURS`.
- Cookie-authenticated `POST`/`PUT`/`PATCH`/`DELETE` requests must carry `x-oryn-csrf`. Cross-site forms cannot set headers, and cross-origin scripts cannot without a CORS preflight the API refuses by default.
- `oryn_pat_` tokens: SHA-256 stored, bound to one workspace, with a role. Effective role = min(token role, creator's current membership role), recomputed per request. Tokens cannot mint tokens or create workspaces. Optional expiry.
- Workers use separate `oryn_wrk_` tokens accepted only on worker endpoints.
- Both client types resolve to the same `Authed` extractor and the same `ws_ctx`/`project_ctx` permission checks.

### Consequences

- Good: logout and token revocation take effect on the next request; demoting a user immediately limits their tokens.
- Good: the web app needs no token handling; it sends one constant header.
- Bad: every request costs a database lookup for the session or token.
- Bad: the CSRF defence relies on CORS staying closed; enabling `ORYN_CORS_ORIGINS` for an untrusted origin would weaken it.

## Pros and cons of the other options

- **JWTs**: stateless, but revocation needs a deny list, and storage in JavaScript-readable places exposes them to XSS.
- **Synchronizer token**: robust CSRF defence independent of CORS, but needs per-session token management in the web app for little gain given `SameSite=Lax` and a closed CORS policy.
