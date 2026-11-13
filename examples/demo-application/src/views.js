// Server-rendered HTML views. Every interpolated value goes through esc().

import { formatMoney } from './pricing.js';

export function esc(value) {
  return String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

const raw = (html) => ({ __html: html });
const render = (v) => (v && typeof v === 'object' && '__html' in v ? v.__html : esc(v));

/** Tagged template: escapes interpolations unless wrapped with raw(). Arrays are joined. */
export function html(strings, ...values) {
  let out = strings[0];
  values.forEach((v, i) => {
    out += Array.isArray(v) ? v.map(render).join('') : render(v);
    out += strings[i + 1];
  });
  return raw(out);
}

export function layout({ title, user, cartCount = 0, flash = null, body }) {
  const account = user
    ? html`<a href="/dashboard">Dashboard</a>
        <form method="post" action="/logout" class="inline"><button type="submit" class="link-button">Log out</button></form>`
    : html`<a href="/login">Log in</a> <a href="/register">Register</a>`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)} | Northwind Supply</title>
<link rel="stylesheet" href="/static/styles.css">
</head>
<body>
<header class="site-header">
  <a class="brand" href="/">Northwind Supply</a>
  <nav aria-label="Main">
    <a href="/products">Products</a>
    <a href="/cart">Cart (<span data-testid="cart-count">${cartCount}</span>)</a>
    ${render(account)}
  </nav>
</header>
<main id="main">
${flash ? `<p class="flash flash-${esc(flash.kind)}" role="status">${esc(flash.message)}</p>` : ''}
${render(body)}
</main>
<footer class="site-footer">Northwind Supply is a demo application for ORYN. Nothing here is for sale.</footer>
<script src="/static/app.js" defer></script>
</body>
</html>`;
}

export const homePage = ({ user, featured }) => html`
<section class="hero">
  <h1>Tools and supplies for every job</h1>
  <p>Hand tools, power tools, fasteners and safety gear. Free shipping on orders of $100 or more.</p>
  <p><a class="button" href="/products">Browse products</a></p>
</section>
${user ? html`<p>Signed in as ${user.name}.</p>` : ''}
<h2>Featured</h2>
<ul class="grid">
  ${featured.map((p) => productCard(p))}
</ul>`;

const productCard = (p) => html`
<li class="card">
  <h3><a href="/products/${p.id}">${p.name}</a></h3>
  <p class="muted">${p.category}</p>
  <p class="price">${formatMoney(p.price)}</p>
  <form method="post" action="/cart/add">
    <input type="hidden" name="product_id" value="${p.id}">
    <input type="hidden" name="quantity" value="1">
    <button type="submit" aria-label="Add ${p.name} to cart">Add to cart</button>
  </form>
</li>`;

export const productsPage = ({ products, q }) => html`
<h1>Products</h1>
<form method="get" action="/products" role="search" class="search">
  <label for="q">Search products</label>
  <input id="q" name="q" type="search" placeholder="Search products" value="${q ?? ''}">
  <button type="submit">Search</button>
</form>
${q ? html`<p class="muted" data-testid="result-count">${products.length} result${products.length === 1 ? '' : 's'} for "${q}"</p>` : ''}
${products.length === 0 ? html`<p>No products found.</p>` : ''}
<ul class="grid">
  ${products.map((p) => productCard(p))}
</ul>`;

export const productPage = ({ product: p }) => html`
<p><a href="/products">&larr; All products</a></p>
<article class="product">
  <h1>${p.name}</h1>
  <p class="muted">SKU ${p.sku} &middot; ${p.category}</p>
  <p>${p.description}</p>
  <p class="price" data-testid="price">${formatMoney(p.price)}</p>
  <p class="muted">${p.stock} in stock</p>
  <form method="post" action="/cart/add" class="stack">
    <input type="hidden" name="product_id" value="${p.id}">
    <label for="quantity">Quantity</label>
    <input id="quantity" name="quantity" type="number" min="1" max="99" value="1" required>
    <button type="submit">Add to cart</button>
  </form>
</article>`;

export const cartPage = ({ lines, totals }) =>
  lines.length === 0
    ? html`<h1>Your cart</h1><p>Your cart is empty.</p><p><a href="/products">Continue shopping</a></p>`
    : html`
<h1>Your cart</h1>
<table class="cart">
  <thead><tr><th scope="col">Product</th><th scope="col">Price</th><th scope="col">Quantity</th><th scope="col">Line total</th><th scope="col"><span class="sr-only">Actions</span></th></tr></thead>
  <tbody>
  ${lines.map(
    (l) => html`<tr data-testid="cart-line-${l.product_id}">
      <td><a href="/products/${l.product_id}">${l.name}</a></td>
      <td>${formatMoney(l.unit_price)}</td>
      <td>
        <form method="post" action="/cart/update" class="inline">
          <input type="hidden" name="product_id" value="${l.product_id}">
          <label class="sr-only" for="qty-${l.product_id}">Quantity for ${l.name}</label>
          <input id="qty-${l.product_id}" name="quantity" type="number" min="1" max="99" value="${l.quantity}">
          <button type="submit">Update</button>
        </form>
      </td>
      <td>${formatMoney(l.line_total)}</td>
      <td>
        <form method="post" action="/cart/remove" class="inline">
          <input type="hidden" name="product_id" value="${l.product_id}">
          <button type="submit" aria-label="Remove ${l.name}">Remove</button>
        </form>
      </td>
    </tr>`,
  )}
  </tbody>
</table>
<p class="subtotal" data-testid="subtotal">Subtotal: ${formatMoney(totals.subtotal)}</p>
<p class="muted">Tax and shipping are calculated at checkout.</p>
<p><a class="button" href="/checkout">Checkout</a></p>`;

export const checkoutPage = ({ user, lines, totals, error }) => html`
<h1>Checkout</h1>
${error ? html`<p class="flash flash-error" role="alert">${error}</p>` : ''}
<section class="summary" aria-label="Order summary">
  <ul>${lines.map((l) => html`<li>${l.quantity} &times; ${l.name} &mdash; ${formatMoney(l.line_total)}</li>`)}</ul>
  <dl>
    <dt>Subtotal</dt><dd data-testid="checkout-subtotal">${formatMoney(totals.subtotal)}</dd>
    <dt>Tax (8%)</dt><dd data-testid="checkout-tax">${formatMoney(totals.tax)}</dd>
    <dt>Shipping</dt><dd data-testid="checkout-shipping">${totals.shipping === 0 ? 'Free' : formatMoney(totals.shipping)}</dd>
    <dt>Total</dt><dd data-testid="checkout-total">${formatMoney(totals.total)}</dd>
  </dl>
</section>
<form method="post" action="/checkout" class="stack">
  <label for="shipping_name">Full name</label>
  <input id="shipping_name" name="shipping_name" autocomplete="name" value="${user.name}" required>
  <label for="shipping_address">Shipping address</label>
  <textarea id="shipping_address" name="shipping_address" autocomplete="street-address" rows="3" required></textarea>
  <button type="submit">Place order</button>
</form>`;

export const orderPage = ({ order }) => html`
<h1>Order confirmed</h1>
<p>Thank you! Your order <strong data-testid="order-id">${order.id}</strong> has been placed.</p>
<p>Shipping to ${order.shipping_name}, ${order.shipping_address}</p>
<table class="cart">
  <tbody>
  ${order.items.map((l) => html`<tr><td>${l.quantity} &times; ${l.name}</td><td>${formatMoney(l.line_total)}</td></tr>`)}
  </tbody>
</table>
<dl class="totals">
  <dt>Subtotal</dt><dd>${formatMoney(order.subtotal)}</dd>
  <dt>Tax</dt><dd>${formatMoney(order.tax)}</dd>
  <dt>Shipping</dt><dd>${order.shipping === 0 ? 'Free' : formatMoney(order.shipping)}</dd>
</dl>
<p class="order-total" data-testid="order-total">Total: ${formatMoney(order.total)}</p>
<p><a href="/dashboard">Back to your dashboard</a></p>`;

export const loginPage = ({ error, email, next }) => html`
<h1>Log in</h1>
${error ? html`<p class="flash flash-error" role="alert">${error}</p>` : ''}
<form method="post" action="/login" class="stack narrow">
  <input type="hidden" name="next" value="${next ?? ''}">
  <label for="email">Email</label>
  <input id="email" name="email" type="email" autocomplete="username" value="${email ?? ''}" required>
  <label for="password">Password</label>
  <input id="password" name="password" type="password" autocomplete="current-password" required>
  <button type="submit">Sign in</button>
</form>
<p><a href="/forgot-password">Forgot your password?</a></p>
<p>New here? <a href="/register">Create an account</a></p>`;

export const registerPage = ({ error, values = {} }) => html`
<h1>Create an account</h1>
${error ? html`<p class="flash flash-error" role="alert">${error}</p>` : ''}
<form method="post" action="/register" class="stack narrow">
  <label for="name">Name</label>
  <input id="name" name="name" autocomplete="name" value="${values.name ?? ''}" required>
  <label for="email">Email</label>
  <input id="email" name="email" type="email" autocomplete="email" value="${values.email ?? ''}" required>
  <label for="password">Password</label>
  <input id="password" name="password" type="password" autocomplete="new-password" minlength="8" required>
  <p class="muted">At least 8 characters.</p>
  <button type="submit">Create account</button>
</form>
<p>Already have an account? <a href="/login">Log in</a></p>`;

export const forgotPasswordPage = ({ sent, error }) => html`
<h1>Reset your password</h1>
${error ? html`<p class="flash flash-error" role="alert">${error}</p>` : ''}
${
  sent
    ? html`<p class="flash flash-success" role="status">If an account exist, a reset link has been sent.</p>`
    : html`<form method="post" action="/forgot-password" class="stack narrow">
  <label for="email">Email</label>
  <input id="email" name="email" type="email" autocomplete="email" required>
  <button type="submit">Send reset link</button>
</form>`
}
<p><a href="/login">Back to log in</a></p>`;

export const resetPasswordPage = ({ token, error, done }) => html`
<h1>Choose a new password</h1>
${error ? html`<p class="flash flash-error" role="alert">${error}</p>` : ''}
${
  done
    ? html`<p class="flash flash-success" role="status">Your password has been updated. You can now log in.</p><p><a href="/login">Log in</a></p>`
    : html`<form method="post" action="/reset-password" class="stack narrow">
  <input type="hidden" name="token" value="${token ?? ''}">
  <label for="new_password">New password</label>
  <input id="new_password" name="new_password" type="password" autocomplete="new-password" minlength="8" required>
  <button type="submit">Update password</button>
</form>`
}`;

export const dashboardPage = ({ user, orders }) => html`
<h1>Welcome, ${user.name}</h1>
<p class="muted">${user.email}</p>
<h2>Recent orders</h2>
${
  orders.length === 0
    ? html`<p>You have no orders yet. <a href="/products">Start shopping</a>.</p>`
    : html`<table class="cart">
  <thead><tr><th scope="col">Order</th><th scope="col">Placed</th><th scope="col">Total</th></tr></thead>
  <tbody>${orders.map(
    (o) => html`<tr><td><a href="/orders/${o.id}">${o.id}</a></td><td>${o.created_at.slice(0, 10)}</td><td>${formatMoney(o.total)}</td></tr>`,
  )}</tbody>
</table>`
}`;

export const notFoundPage = () => html`<h1>Page not found</h1><p>We couldn't find that page. <a href="/">Go home</a>.</p>`;
