# Northwind Supply — Requirements

Northwind Supply is an online hardware store. This document is the product
specification that ORYN's planner reads to derive test scenarios. Each user
story has an identifier (`US-n`) and testable acceptance criteria (`AC-n.m`).

Seed data available in every environment:

| Item | Value |
| --- | --- |
| Demo customer | `demo@northwind.test` / `demo-password` (name "Demo Customer") |
| Cheapest-but-one product | Claw Hammer (`claw-hammer`), $12.50 |
| Premium product | Aluminium Step Ladder (`step-ladder`), $119.00 |
| Catalogue size | 8 products |

---

## US-1 Registration

**As a** new visitor **I want to** create an account **so that** I can place orders.

- **AC-1.1** The page `/register` has fields labelled **Name**, **Email** and **Password** and a **Create account** button.
- **AC-1.2** Submitting valid details creates the account, signs the user in and redirects to `/dashboard`.
- **AC-1.3** The dashboard greets the user with **"Welcome, &lt;name&gt;"**.
- **AC-1.4** Passwords shorter than 8 characters are rejected with "Password must be at least 8 characters".
- **AC-1.5** Registering an email that already exists is rejected with "An account with that email already exists" (API: HTTP 409).

## US-2 Login

**As a** returning customer **I want to** sign in **so that** I can see my orders and check out.

- **AC-2.1** The page `/login` has fields labelled **Email** and **Password** and a **Sign in** button.
- **AC-2.2** Valid credentials redirect to `/dashboard`, which shows "Welcome, &lt;name&gt;" and the customer's recent orders.
- **AC-2.3** The session is held in an HttpOnly cookie; **Log out** ends the session and returns to the home page.
- **AC-2.4** `POST /api/auth/login` returns `{ token, user }`; the token is accepted as `Authorization: Bearer <token>` on the API.
- **AC-2.5** `/dashboard`, `/checkout` and `/orders/:id` require login and redirect anonymous visitors to `/login`.

## US-3 Invalid login

**As a** store operator **I want** failed sign-ins to reveal nothing **so that** accounts cannot be enumerated.

- **AC-3.1** A wrong password or unknown email shows exactly **"Invalid email or password"** and stays on `/login`.
- **AC-3.2** The API responds `401` with `{"error":{"code":"invalid_credentials", ...}}`.

## US-4 Password reset

**As a** customer who forgot my password **I want to** reset it by email.

- **AC-4.1** `/login` links to **Forgot your password?** (`/forgot-password`).
- **AC-4.2** Submitting any well-formed email shows exactly: **"If an account exists, a reset link has been sent."** The message is the same whether or not the account exists.
- **AC-4.3** For an existing account a single-use reset link `/reset-password?token=…` valid for 30 minutes is e-mailed (demo: visible at `GET /api/__outbox`).
- **AC-4.4** The reset page has a **New password** field and an **Update password** button; after success the user can sign in with the new password.
- **AC-4.5** An invalid or expired token shows "This reset link is invalid or has expired."

## US-5 Browse catalogue

**As a** shopper **I want to** browse products.

- **AC-5.1** `/products` lists all 8 products with name, category, price and an **Add to cart** button each.
- **AC-5.2** Each product links to `/products/:id`, which shows the SKU, description, price, stock and a **Quantity** field.
- **AC-5.3** An unknown product id shows "Page not found" (API: `404` with `{"error":{"code":"not_found"}}`).
- **AC-5.4** The page title of `/products` is **"Products | Northwind Supply"**.

## US-6 Search

**As a** shopper **I want to** search the catalogue.

- **AC-6.1** The **Search products** box on `/products` filters by name, category, description or SKU, case-insensitively.
- **AC-6.2** Results show "&lt;n&gt; result(s) for "&lt;term&gt;"". No match shows "No products found."
- **AC-6.3** `GET /api/products?q=<term>` returns `{ products: [...], count }` and must respond within 500 ms.
- **AC-6.4** Search must be reliable: a search request must not fail for a healthy service.

## US-7 Cart

**As a** shopper **I want to** collect products in a cart.

- **AC-7.1** Adding a product redirects to `/cart` and shows "Added &lt;product&gt; to your cart."
- **AC-7.2** The cart lists each line with unit price, quantity and line total, and shows **"Subtotal: $x.xx"** and a **Checkout** link.
- **AC-7.3** Adding the same product again increases the quantity. A line's quantity must be between 1 and 99; the API rejects other values with `400` and an `error` object.
- **AC-7.4** The header shows the number of items in the cart.
- **AC-7.5** Products can be removed (`DELETE /api/cart/items/:product_id`).

## US-8 Checkout and totals

**As a** customer **I want** correct totals **so that** I am charged the right amount.

- **AC-8.1** subtotal = Σ (unit price × quantity) over all cart lines.
- **AC-8.2** tax = 8% of the subtotal, rounded to the nearest cent.
- **AC-8.3** shipping = **free** when the subtotal is **$100.00 or more**, otherwise **$7.50**.
- **AC-8.4** total = subtotal + tax + shipping, rounded to cents.
- **AC-8.5** `/checkout` shows the summary and asks for **Full name** and **Shipping address**; **Place order** redirects to `/orders/:id`, which shows **"Order confirmed"** and **"Total: $x.xx"**.
- **AC-8.6** `POST /api/checkout` returns `{ order_id, subtotal, tax, shipping, total, currency }` and empties the cart; checking out an empty cart returns `400` (`empty_cart`).

Worked examples (normative):

| Cart | Subtotal | Tax | Shipping | Total |
| --- | --- | --- | --- | --- |
| 1 × Claw Hammer ($12.50) | $12.50 | $1.00 | $7.50 | **$21.00** |
| 2 × Claw Hammer ($12.50) | $25.00 | $2.00 | $7.50 | **$34.50** |
| 1 × Cordless Drill ($89.00) | $89.00 | $7.12 | $7.50 | **$103.62** |
| 2 × Socket Set ($54.00) | $108.00 | $8.64 | free | **$116.64** |
| 1 × Step Ladder ($119.00) | $119.00 | $9.52 | free | **$128.52** |
