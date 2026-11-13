# Northwind Supply (ORYN demo application)

Northwind Supply is a small, realistic hardware-store web application used to
demonstrate ORYN. It has server-rendered pages (registration, login, password
reset, catalogue, search, cart, checkout, orders) and a JSON REST API with an
OpenAPI document. It contains **three planted defects** so that ORYN has
something real to find.

- Zero runtime dependencies: plain `node:http`, ES modules, Node 22+.
- All state is in memory. It resets on restart or with `POST /api/__reset`.
- Licence: Apache-2.0 (same as ORYN).

## Run it

```bash
cd examples/demo-application
npm start              # http://localhost:4000
npm run dev            # same, restarts on file changes (node --watch)
PORT=4100 npm start    # different port
DEMO_FLAKY=0 npm start # turn off the flaky-search defect
npm test               # unit + API smoke tests (node:test)
```

With Docker:

```bash
docker build -t northwind-supply examples/demo-application
docker run --rm -p 4000:4000 northwind-supply
```

### Seed account

| Email | Password |
| --- | --- |
| `demo@northwind.test` | `demo-password` |

### Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `PORT` | `4000` | Listen port |
| `HOST` | all interfaces | Bind address |
| `DEMO_FLAKY` | `1` | `1` enables the flaky search defect, `0` disables it |

## Pages

| Path | Purpose |
| --- | --- |
| `/` | Home page with navigation (Products, Cart, Log in / Dashboard) |
| `/register` | Name, Email, Password, **Create account** → `/dashboard` ("Welcome, &lt;name&gt;") |
| `/login` | Email, Password, **Sign in**; wrong credentials → "Invalid email or password" |
| `/forgot-password` | Email, **Send reset link** |
| `/reset-password?token=…` | New password, **Update password** |
| `/dashboard` | Requires login. Greeting and recent orders |
| `/products`, `/products/:id` | Catalogue with search box and **Add to cart** buttons; product detail with Quantity |
| `/cart` | Line items, quantities, subtotal, **Checkout** |
| `/checkout` | Requires login. Full name, Shipping address, **Place order** → `/orders/:id` ("Order confirmed", "Total: $x.xx") |
| `/logout` | POST (the header button) or GET |

The session is an HttpOnly cookie (`nw_session`).

## REST API

The full contract is in [`openapi.json`](openapi.json), which is also served at
`GET /openapi.json`. Errors always look like
`{"error": {"code": "...", "message": "..."}}`.

| Method & path | Notes |
| --- | --- |
| `GET /api/health` | `{"status":"ok"}` |
| `POST /api/auth/register` | `{name, email, password}` → 201 `{token, user}`; 400 validation, 409 duplicate |
| `POST /api/auth/login` | `{email, password}` → `{token, user}`; 401 on bad credentials |
| `POST /api/auth/forgot-password` | `{email}` → 202 |
| `GET /api/products?q=` | `{products, count}` |
| `GET /api/products/:id` | 404 JSON for unknown ids |
| `GET /api/cart` | Requires `Authorization: Bearer <token>` (or the session cookie) |
| `POST /api/cart/items` | `{product_id, quantity}`; 400 when quantity < 1 or > 99 |
| `DELETE /api/cart/items/:product_id` | |
| `POST /api/checkout` | Optional `{shipping_name, shipping_address}` → 201 `{order_id, subtotal, tax, shipping, total, currency}` |
| `GET /api/orders/:id` | Owner only |
| `POST /api/__reset` | Test helper: remove test-created accounts, their sessions and orders, reset tokens and sent e-mail. Sessions of the seeded customer survive, so concurrently running scenarios do not sign each other out |
| `GET /api/__outbox` | Test helper: e-mails the app has "sent" (password reset links) |

Example:

```bash
TOKEN=$(curl -s -X POST localhost:4000/api/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"demo@northwind.test","password":"demo-password"}' | jq -r .token)
curl -s -X POST localhost:4000/api/cart/items -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{"product_id":"claw-hammer","quantity":2}'
curl -s -X POST localhost:4000/api/checkout -H "authorization: Bearer $TOKEN"
```

## Specification and example tests

- [`docs/requirements.md`](docs/requirements.md): eight user stories with acceptance criteria. ORYN's planner reads this.
- [`features/`](features): Gherkin features written only with ORYN's built-in step vocabulary.
  `@browser` features drive a real browser through the Playwright runner; `@api` features call the REST API.

| Feature | Expected result on the unfixed app |
| --- | --- |
| `auth.feature` | Passes (on a fresh state; registration uses a fixed email, so reset between runs) |
| `password_reset.feature` | **Fails**: defect 3 |
| `catalogue.feature` | Passes |
| `cart_checkout.feature` | **Fails** "Buy two of a cheap item": defect 1 |
| `api_products.feature` | "Search the catalogue" **fails intermittently**: defect 2 |
| `api_checkout.feature` | **Fails** "Two of a cheap item are priced per the spec": defect 1 |

## Planted defects

These are deliberate. Each one is a different kind of failure that ORYN should
classify differently.

### 1. Pricing bug (application defect, deterministic)

**Where:** `src/pricing.js`, `qualifyingValueCents()`.

**Symptom:** any order containing a line with quantity > 1 is priced wrongly.
Tax and the free-shipping threshold are computed from the sum of *unit prices*,
ignoring quantities, while the subtotal is correct. For example 2 × Claw Hammer
($12.50) gives tax $1.00 and total **$33.50** instead of tax $2.00 and total
**$34.50**; 2 × Socket Set ($54.00, subtotal $108.00) is charged $7.50
shipping, which should be free.

**Spec:** `docs/requirements.md` US-8 (AC-8.2, AC-8.3 and the worked examples).

**Fix:** use the line subtotal (unit price × quantity) for the qualifying value:

```js
function qualifyingValueCents(lines) {
  return lines.reduce((sum, line) => sum + lineTotalCents(line), 0);
}
```

After the fix, remove `{ todo: 'planted defect' }` from the last pricing test
in `test/pricing.test.js`; it then passes as a normal test.

### 2. Flaky search (environment / flaky defect, intermittent)

**Where:** `src/server.js`, the `GET /api/products` handler.

**Symptom:** when a `q` parameter is present and `DEMO_FLAKY=1` (the default),
about 25% of requests fail with `503 {"error":{"code":"search_unavailable"}}`.
Re-running the same request usually succeeds. This models an overloaded
dependency rather than a logic error, so ORYN should flag it as flaky, not as a
regression.

**Spec:** `docs/requirements.md` AC-6.4.

**Fix:** start the app with `DEMO_FLAKY=0`, or delete the `if (q !== null && flaky ...)` block.

### 3. Password reset message typo (application defect, deterministic)

**Where:** `src/views.js`, `forgotPasswordPage()`.

**Symptom:** after submitting the forgot-password form the page says
"If an account **exist**, a reset link has been sent." The spec requires
"If an account **exists**, a reset link has been sent." A text assertion
(`I should see "If an account exists, a reset link has been sent."`) fails.
The API (`POST /api/auth/forgot-password`) returns the correct text.

**Spec:** `docs/requirements.md` AC-4.2.

**Fix:** change `exist` to `exists` in the success message.
