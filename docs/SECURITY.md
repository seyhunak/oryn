# Security model

This page describes the threat model and the controls that exist in the code. It ends with the known limitations. To report a vulnerability, see the root [`SECURITY.md`](../SECURITY.md).

## Assets and trust boundaries

| Asset | Where it lives |
|---|---|
| User passwords | `users.password_hash` (Argon2id) |
| Session and API tokens | `sessions.token_hash`, `api_tokens.token_hash` (SHA-256 of the token) |
| Worker tokens | `workers.token_hash` (SHA-256) |
| Environment secrets, repository tokens, AI provider keys, webhook secrets | Encrypted columns (`*_enc`), AES-256-GCM |
| Source code of imported repositories | `ORYN_WORKSPACE_DIR` (working copies) and `code_chunks` (redacted text) |
| Evidence (screenshots, video, traces, logs) | `ORYN_ARTIFACT_DIR` on the API host |

Untrusted inputs, in decreasing order of exposure:

1. HTTP requests from browsers and API clients.
2. Repository content (ZIP uploads, git clones), requirement documents and Gherkin text.
3. The application under test: page content, HTTP responses, redirects.
4. Model output from AI providers.
5. Workers. A worker holds a token and receives job specs with decrypted secrets, so it is trusted with those secrets, but its reports are validated (artifact types and sizes, job ownership, lease).

## Authentication

**Passwords.** Argon2id with the `argon2` crate defaults. Length 10–256. Login compares against a fixed dummy hash when the account does not exist, so response time does not reveal which emails are registered.

**Brute force.** `POST /auth/login` is limited to 30 requests per 5 minutes per client IP. Independently, an account is locked after 10 failed attempts within 15 minutes; only failures increment the counter, and the counter is checked before the password is verified. Registration is limited to 10 per 10 minutes per IP. Limits are fixed-window and use Redis when `REDIS_URL` is set, otherwise in-process memory (per API replica).

**Sessions.** A random token is returned in the `oryn_session` cookie (`HttpOnly`, `SameSite=Lax`, `Path=/`, `Secure` when `ORYN_COOKIE_SECURE=true`); only its SHA-256 is stored. Lifetime `ORYN_SESSION_TTL_HOURS` (default 168). Expired sessions are purged by the background task about hourly.

**CSRF.** Cookie-authenticated requests with a state-changing method must carry the `x-oryn-csrf` header. Browsers cannot add custom headers to cross-site form posts, and cross-origin `fetch` with custom headers needs a CORS preflight that the API refuses unless the origin is in `ORYN_CORS_ORIGINS`. Combined with `SameSite=Lax`, this blocks cross-site request forgery. Only the header's presence is checked.

**API tokens.** Prefixed `oryn_pat_`, bound to one workspace and given a role. The effective role is the lower of the token's role and the creator's current membership role, so a token never outlives a demotion. Tokens cannot create tokens or workspaces. Expiry is optional, up to 3650 days. The secret is shown once.

**Worker tokens.** Prefixed `oryn_wrk_`. Workspace workers are created by an admin; shared workers self-register with `ORYN_WORKER_BOOTSTRAP_TOKEN` (compared in constant time, rate-limited to 60 per minute). Revoking a worker invalidates its token immediately and requeues its leased jobs.

**OIDC.** Authorization-code flow with PKCE (S256), a random `state` and a `nonce`, stored server-side for 15 minutes. The `next` parameter must be a relative path, which prevents open redirects. The ID token is required and its `iss`, `aud`, `azp`, `exp` (60 s skew) and `nonce` are checked; its signature is not, because ORYN receives it directly from the issuer's token endpoint over TLS (OIDC Core 3.1.3.7, item 6). The userinfo `sub` must equal the ID token's. The provider must assert `email_verified: true`. A first sign-in links to a local account with the same email only if that account has no OIDC identity yet, and creates an account only when `ORYN_ALLOW_REGISTRATION` is on.

**Client IP.** Rate-limit keys and audit records use the TCP peer address. `X-Forwarded-For` is believed only when the peer is in `ORYN_TRUSTED_PROXIES` (default: loopback and private ranges, which covers the bundled web server and a sidecar proxy; `none` disables it), and then the right-most address that is not itself a trusted proxy is used.

## RBAC and tenant isolation

Roles are `owner`, `admin`, `engineer` and `viewer`. The matrix lives in `crates/oryn-domain/src/rbac.rs` and is served at `GET /api/v1/roles`; [API_SPEC.md](API_SPEC.md#authorisation) reproduces it. A member can assign only roles at or below their own, and the last owner of a workspace cannot be removed or demoted.

Every workspace-scoped handler resolves the caller's membership first (`ws_ctx`). Every project-scoped handler resolves the project, then the caller's project role (`project_ctx`):

- Workspace owners and admins get their workspace role on every project.
- On a workspace-visible project, other members get the higher of their workspace role and any explicit project grant (user or team).
- On a private project, a member needs an explicit grant.
- For API tokens the result is capped at the token's role.

Resources outside the caller's reach return `404`, so IDs in other workspaces cannot be probed. All database queries for tenant data filter on workspace or project id.

## Secrets

`SecretBox` derives a 256-bit key as SHA-256 of `ORYN_SECRET_KEY` (minimum 32 characters, enforced at start-up) and stores `v1:` + base64(nonce ‖ ciphertext) using AES-256-GCM with a random 96-bit nonce. Encrypted values are never returned by the API; listings show secret names, masked provider keys or `has_secret` flags.

Environment secrets are decrypted only when a worker claims a job, sent in the claim response over the worker connection, and merged into step variables by the worker. Changing `ORYN_SECRET_KEY` makes every stored secret unreadable; there is no key rotation command.

## Redaction

`oryn_auth::redact` masks bearer tokens, `sk-` keys, GitHub (`gh*_`) and GitLab (`glpat-`) tokens, AWS access key IDs, ORYN tokens, JWTs, `password`/`secret`/`api_key`/`token` key-value pairs and card numbers. `redact_with` additionally replaces the exact values of a job's secrets (4 characters or longer). It is applied to:

- step logs, step errors, network records and the job error before they are stored;
- analysed source text before it is chunked and indexed;
- every prompt and every model response that passes through the model gateway;
- git stderr in import errors.

The Playwright runner applies the same rules to its own output, and recordings replace typed passwords with `{{password}}`.

## Outbound requests and SSRF

ORYN makes outbound requests on behalf of users in five places. Each has its own guard:

| Caller | Guard |
|---|---|
| Webhook and Slack deliveries (`services/notify.rs`) | `https` only (`http` allowed when `ORYN_ALLOW_PRIVATE_NETWORK=true`). The host is resolved and every address is checked: loopback, private, link-local, CGNAT, unspecified, broadcast, unique-local and IPv4-mapped IPv6 are refused unless private networks are allowed; cloud metadata addresses are always refused. Slack URLs must be on `hooks.slack.com`. 15 s timeout, no redirects. |
| Git imports (`services/import.rs`) | `https` only (`http` when private networks are allowed), no credentials in the URL, metadata hosts refused, private host literals refused unless allowed. Clones run with hooks disabled, only http(s) transports, no submodules, no symlinks, shallow, 300 s timeout; the access token is passed through `GIT_CONFIG_*` environment variables, never on the command line. |
| Repository connections | The import URL host must equal the connection host, so a stored token is never sent to another server. |
| AI providers | Metadata hostnames refused in `base_url`. |
| Test steps (API engine and browser) | Requests are limited to the environment's allowed hosts (the `allowed_hosts` list, or the base URL hosts when the list is empty). Cloud metadata hosts are always blocked. |

`ORYN_ALLOW_PRIVATE_NETWORK` defaults to `false` and is set to `true` in the development `Makefile` and `docker-compose.yml` so local targets work.

## Imports

ZIP uploads are validated in two passes before anything is written: at most 20 000 entries, 500 MB uncompressed in total, 50 MB per file, compression ratio at most 200 for files over 1 MB, no symlinks, no absolute paths or `..` components. Decompressed bytes are counted while extracting, so a lying header cannot exceed the limits. Local-directory imports are canonicalised and must stay under `ORYN_LOCAL_IMPORT_ROOT`; they are read, never modified or deleted. Analysis reads at most 1 MB of each file and 25 000 files.

## AI

The gateway ([ARCHITECTURE.md](ARCHITECTURE.md#model-gateway)) treats repository content, requirements and evidence as untrusted:

- Untrusted text is redacted and wrapped in a fenced block, with a safety preamble telling the model that text inside the fence is data, not instructions.
- Every response must validate against a JSON schema; invalid output is repaired once, then rejected.
- The model has no tools: no shell, no file system, no network.
- Model output never becomes a verdict. A test passes or fails only on deterministic assertions; AI-generated test plans are proposals until a person with `test:approve` approves them; AI investigation text is stored separately and labelled "AI interpretation".
- AI is optional and configured per workspace. Every call is recorded in `ai_requests` with tokens and cost.

## Evidence serving

`GET /artifacts/{id}/content` requires `project:read` on the artifact's project, serves only images, video, JSON and text inline (everything else as an `application/octet-stream` attachment), and sets `Content-Security-Policy: sandbox; default-src 'none'; img-src 'self'; media-src 'self'` and `X-Content-Type-Options: nosniff`. Uploaded artifact names are sanitised and stored under `{project}/{execution}/{job}/`.

## HTTP hardening

All API responses carry `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY` and `Referrer-Policy: no-referrer`; the web app sets the same. `Authorization` and `Cookie` are marked sensitive in traces. Bodies are capped at `ORYN_MAX_UPLOAD_BYTES`; requests time out after 300 s. Containers run as a non-root user (uid 10001 for API and worker, `pwuser` for the runner).

## Audit

`audit_log` records sign-ins (including failures) and registrations; workspace, member, team, token and project-permission changes; environment (secret) and repository-connection changes; AI provider and integration changes; repository imports; requirement, suite, scenario-status and plan changes; execution creation, cancellation and regression runs; worker creation and revocation. Each entry has the actor (user or token), IP and user agent. Readable by `audit:read` at `GET /workspaces/{ws}/audit`.

## Outbound requests to user-chosen hosts

Webhooks, Slack, repository hosts (clones and connection checks) and AI provider endpoints go through `crates/oryn-auth/src/net.rs`. A host is resolved once; if any of its addresses is a cloud metadata address, or a private, loopback, link-local or CGNAT address while `ORYN_ALLOW_PRIVATE_NETWORK` is off, the whole host is refused. HTTP clients use this as their DNS resolver, so they connect only to the addresses that were checked: there is no second lookup for DNS rebinding to exploit. IP-literal hosts, which skip resolution, are checked when the URL is validated. Git clones are pinned to the checked addresses with `http.curloptResolve`, and redirects are not followed on any of these paths. A local Ollama or a self-hosted GitLab on a private network therefore needs `ORYN_ALLOW_PRIVATE_NETWORK=true`.

## Known limitations

These follow from the code as it stands and are tracked in [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md#phase-3-hardening).

1. **Test traffic may reach private addresses.** Test environments are usually internal, so test steps are restricted by host (the environment's own hosts plus its `allowed_hosts`, with `*.domain` wildcards, and every redirect hop re-checked), not by address range. Cloud metadata hosts are always blocked, by name, in both the API engine and the browser runner.
2. **Shared workers receive secrets from every workspace** whose jobs they claim. Use workspace workers when tenants must not share execution hosts.
3. **Prompt fencing reduces, but cannot eliminate, prompt injection.** All user- and repository-supplied text is fenced and fence tags inside it are defused in any spelling, and model output is schema-validated data that never runs and never sets a verdict; a model may still be misled within those bounds.
4. **Audit writes are best-effort** (a failed insert is logged, not fatal), and login events have no workspace, so they do not appear in a workspace audit view.
5. **No secret key rotation**, and artifacts are stored unencrypted on the API host's file system.
