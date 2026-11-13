// API smoke tests against an in-process server on an ephemeral port.
import { test, before, after, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { startServer } from '../src/server.js';

let server;
let base;

before(async () => {
  server = await startServer({ port: 0, host: '127.0.0.1', flaky: false, log: () => {} });
  base = `http://127.0.0.1:${server.address().port}`;
});
after(() => new Promise((resolve) => server.close(resolve)));
beforeEach(() => fetch(`${base}/api/__reset`, { method: 'POST' }));

const api = async (method, path, { body, token } = {}) => {
  const headers = {};
  if (body !== undefined) headers['content-type'] = 'application/json';
  if (token) headers.authorization = `Bearer ${token}`;
  const res = await fetch(`${base}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await res.text();
  return { status: res.status, json: text ? JSON.parse(text) : null };
};

const login = async () =>
  (await api('POST', '/api/auth/login', { body: { email: 'demo@northwind.test', password: 'demo-password' } })).json.token;

test('health', async () => {
  const r = await api('GET', '/api/health');
  assert.equal(r.status, 200);
  assert.equal(r.json.status, 'ok');
});

test('catalogue list, search and 404', async () => {
  const all = await api('GET', '/api/products');
  assert.equal(all.status, 200);
  assert.equal(all.json.count, 8);
  const hammers = await api('GET', '/api/products?q=hammer');
  assert.equal(hammers.json.products.length, 1);
  assert.equal(hammers.json.products[0].id, 'claw-hammer');
  const missing = await api('GET', '/api/products/does-not-exist');
  assert.equal(missing.status, 404);
  assert.equal(missing.json.error.code, 'not_found');
});

test('login: success returns token, wrong password is 401', async () => {
  assert.ok(await login());
  const bad = await api('POST', '/api/auth/login', { body: { email: 'demo@northwind.test', password: 'wrong' } });
  assert.equal(bad.status, 401);
  assert.equal(bad.json.error.code, 'invalid_credentials');
});

test('register validates and creates an account', async () => {
  const invalid = await api('POST', '/api/auth/register', { body: { name: '', email: 'x', password: '1' } });
  assert.equal(invalid.status, 400);
  const ok = await api('POST', '/api/auth/register', {
    body: { name: 'Grace', email: 'grace@example.test', password: 'long-enough' },
  });
  assert.equal(ok.status, 201);
  assert.equal(ok.json.user.email, 'grace@example.test');
  const dup = await api('POST', '/api/auth/register', {
    body: { name: 'Grace', email: 'grace@example.test', password: 'long-enough' },
  });
  assert.equal(dup.status, 409);
});

test('cart requires auth and validates quantity', async () => {
  assert.equal((await api('GET', '/api/cart')).status, 401);
  const token = await login();
  for (const quantity of [0, 100, 1.5]) {
    const r = await api('POST', '/api/cart/items', { token, body: { product_id: 'claw-hammer', quantity } });
    assert.equal(r.status, 400, `quantity ${quantity}`);
    assert.ok(r.json.error.code);
  }
  const added = await api('POST', '/api/cart/items', { token, body: { product_id: 'tape-measure', quantity: 3 } });
  assert.equal(added.status, 201);
  assert.equal(added.json.subtotal, 29.97);
  const removed = await api('DELETE', '/api/cart/items/tape-measure', { token });
  assert.equal(removed.status, 200);
  assert.equal(removed.json.items.length, 0);
});

test('checkout of a single item returns an order with correct totals', async () => {
  const token = await login();
  await api('POST', '/api/cart/items', { token, body: { product_id: 'cordless-drill', quantity: 1 } });
  const order = await api('POST', '/api/checkout', { token, body: {} });
  assert.equal(order.status, 201);
  // 89.00 + 7.12 tax + 7.50 shipping
  assert.deepEqual(
    { subtotal: order.json.subtotal, tax: order.json.tax, shipping: order.json.shipping, total: order.json.total },
    { subtotal: 89, tax: 7.12, shipping: 7.5, total: 103.62 },
  );
  const fetched = await api('GET', `/api/orders/${order.json.order_id}`, { token });
  assert.equal(fetched.status, 200);
  assert.equal(fetched.json.total, 103.62);
  const empty = await api('POST', '/api/checkout', { token, body: {} });
  assert.equal(empty.status, 400);
});

test('flaky search fails some requests with 503 when enabled', async () => {
  const flakyServer = await startServer({ port: 0, host: '127.0.0.1', flaky: true, random: () => 0, log: () => {} });
  try {
    const r = await fetch(`http://127.0.0.1:${flakyServer.address().port}/api/products?q=drill`);
    assert.equal(r.status, 503);
  } finally {
    await new Promise((resolve) => flakyServer.close(resolve));
  }
});

test('pages render with accessible forms', async () => {
  const login = await (await fetch(`${base}/login`)).text();
  assert.match(login, /<label for="email">Email<\/label>/);
  assert.match(login, /<button type="submit">Sign in<\/button>/);
  const openapi = await (await fetch(`${base}/openapi.json`)).json();
  assert.equal(openapi.openapi, '3.0.3');
});
