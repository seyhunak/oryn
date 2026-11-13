// Northwind Supply: a small hardware-store demo application for ORYN.
// Zero runtime dependencies; plain node:http, in-memory state.

import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { db, cartLines, publicUser, verifyPassword, SEED_USER } from './store.js';
import { priceOrder } from './pricing.js';
import * as views from './views.js';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const OPENAPI = readFileSync(join(ROOT, 'openapi.json'), 'utf8');
const STATIC = {
  '/static/styles.css': { type: 'text/css; charset=utf-8', body: readFileSync(join(ROOT, 'public/styles.css')) },
  '/static/app.js': { type: 'text/javascript; charset=utf-8', body: readFileSync(join(ROOT, 'public/app.js')) },
};

const SESSION_COOKIE = 'nw_session';
const MAX_BODY = 64 * 1024;
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

class HttpError extends Error {
  constructor(status, code, message) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

// ---------------------------------------------------------------- helpers

function parseCookies(header) {
  const out = {};
  for (const part of (header ?? '').split(';')) {
    const i = part.indexOf('=');
    if (i > 0) out[part.slice(0, i).trim()] = decodeURIComponent(part.slice(i + 1).trim());
  }
  return out;
}

async function readBody(req) {
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > MAX_BODY) throw new HttpError(413, 'payload_too_large', 'Request body too large');
    chunks.push(chunk);
  }
  const text = Buffer.concat(chunks).toString('utf8');
  const type = (req.headers['content-type'] ?? '').split(';')[0].trim();
  if (!text) return {};
  if (type === 'application/json') {
    try {
      const parsed = JSON.parse(text);
      if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
        throw new Error('not an object');
      }
      return parsed;
    } catch {
      throw new HttpError(400, 'invalid_json', 'Request body must be a JSON object');
    }
  }
  return Object.fromEntries(new URLSearchParams(text));
}

function sendJson(res, status, payload, extraHeaders = {}) {
  const body = payload === undefined ? '' : JSON.stringify(payload);
  res.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store',
    ...extraHeaders,
  });
  res.end(body);
}

const apiError = (res, status, code, message) => sendJson(res, status, { error: { code, message } });

function sendHtml(res, status, body, headers = {}) {
  res.writeHead(status, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store', ...headers });
  res.end(body);
}

function redirect(res, location, headers = {}) {
  res.writeHead(303, { location, ...headers });
  res.end();
}

const safeNext = (next) => (typeof next === 'string' && /^\/(?!\/)/.test(next) ? next : null);

// ---------------------------------------------------------------- app

/**
 * @param {{ flaky?: boolean, flakyRate?: number, random?: () => number, log?: (msg: string) => void }} [options]
 */
export function createApp(options = {}) {
  const flaky = options.flaky ?? process.env.DEMO_FLAKY !== '0';
  const flakyRate = options.flakyRate ?? 0.25;
  const random = options.random ?? Math.random;
  const log = options.log ?? ((msg) => console.log(msg));

  /** Resolve (and for browsers, lazily create) the session for a request. */
  function getContext(req, { create = false } = {}) {
    const auth = req.headers.authorization ?? '';
    if (auth.startsWith('Bearer ')) {
      const token = auth.slice(7).trim();
      const session = db.session(token);
      return { sid: session ? token : null, session, viaToken: true, setCookie: null };
    }
    const cookies = parseCookies(req.headers.cookie);
    let sid = cookies[SESSION_COOKIE];
    let session = db.session(sid);
    let setCookie = null;
    if (!session && create) {
      sid = db.createSession();
      session = db.session(sid);
      setCookie = sessionCookie(sid);
    }
    return { sid: session ? sid : null, session, viaToken: false, setCookie };
  }

  const sessionCookie = (sid) => `${SESSION_COOKIE}=${sid}; Path=/; HttpOnly; SameSite=Lax`;
  const clearCookie = `${SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0`;

  const currentUser = (ctx) => (ctx.session?.user_id ? db.user(ctx.session.user_id) : null);
  const cartCount = (ctx) => (ctx.session ? [...ctx.session.cart.values()].reduce((a, b) => a + b, 0) : 0);

  function page(res, ctx, status, title, body, extra = {}) {
    const flash = ctx.session?.flash ?? null;
    if (ctx.session) ctx.session.flash = null;
    const headers = ctx.setCookie ? { 'set-cookie': ctx.setCookie } : {};
    sendHtml(
      res,
      status,
      views.layout({ title, user: currentUser(ctx), cartCount: cartCount(ctx), flash, body, ...extra }),
      headers,
    );
  }

  function cartView(session) {
    const lines = cartLines(session);
    return { lines, totals: priceOrder(lines) };
  }

  function addToCart(session, productId, quantity) {
    const product = db.product(productId);
    if (!product) throw new HttpError(404, 'not_found', `Unknown product: ${productId}`);
    const qty = Number(quantity);
    if (!Number.isInteger(qty) || qty < 1 || qty > 99) {
      throw new HttpError(400, 'invalid_quantity', 'quantity must be an integer between 1 and 99');
    }
    const next = (session.cart.get(product.id) ?? 0) + qty;
    if (next > 99) throw new HttpError(400, 'invalid_quantity', 'A cart line cannot exceed 99 units');
    session.cart.set(product.id, next);
  }

  function placeOrder(session, user, shipping) {
    const lines = cartLines(session);
    if (lines.length === 0) throw new HttpError(400, 'empty_cart', 'Your cart is empty');
    const totals = priceOrder(lines);
    const order = db.createOrder({
      user_id: user.id,
      items: lines,
      shipping_name: shipping.name,
      shipping_address: shipping.address,
      ...totals,
    });
    session.cart.clear();
    return order;
  }

  function startPasswordReset(email) {
    const user = db.userByEmail(email);
    if (user) {
      const token = db.createResetToken(user.id);
      const link = `/reset-password?token=${token}`;
      db.sendMail({ to: user.email, subject: 'Reset your Northwind Supply password', link });
      log(`[mail] password reset link generated for a user (see GET /api/__outbox)`);
    }
  }

  function validateRegistration({ name, email, password }) {
    if (typeof name !== 'string' || name.trim().length === 0) return 'Name is required';
    if (typeof email !== 'string' || !EMAIL_RE.test(email.trim())) return 'A valid email is required';
    if (typeof password !== 'string' || password.length < 8) return 'Password must be at least 8 characters';
    if (db.userByEmail(email)) return 'An account with that email already exists';
    return null;
  }

  const orderJson = (o) => ({
    order_id: o.id,
    status: 'confirmed',
    created_at: o.created_at,
    items: o.items,
    shipping_name: o.shipping_name,
    shipping_address: o.shipping_address,
    subtotal: o.subtotal,
    tax: o.tax,
    shipping: o.shipping,
    total: o.total,
    currency: o.currency,
  });

  // ------------------------------------------------------------ API routes

  async function handleApi(req, res, url) {
    const { pathname } = url;
    const method = req.method;

    if (pathname === '/api/health' && method === 'GET') {
      return sendJson(res, 200, { status: 'ok', service: 'northwind-supply', version: '1.0.0' });
    }
    if (pathname === '/api/__reset' && method === 'POST') {
      db.reset();
      return sendJson(res, 200, { status: 'reset' });
    }
    if (pathname === '/api/__outbox' && method === 'GET') {
      return sendJson(res, 200, { messages: db.outbox() });
    }

    if (pathname === '/api/auth/register' && method === 'POST') {
      const body = await readBody(req);
      const problem = validateRegistration(body);
      if (problem) {
        const conflict = problem.startsWith('An account');
        return apiError(res, conflict ? 409 : 400, conflict ? 'email_taken' : 'validation_error', problem);
      }
      const user = db.createUser(body);
      const token = db.createSession(user.id);
      return sendJson(res, 201, { token, user: publicUser(user) });
    }
    if (pathname === '/api/auth/login' && method === 'POST') {
      const body = await readBody(req);
      if (typeof body.email !== 'string' || typeof body.password !== 'string') {
        return apiError(res, 400, 'validation_error', 'email and password are required');
      }
      const user = db.userByEmail(body.email);
      if (!user || !verifyPassword(body.password, user.password_hash)) {
        return apiError(res, 401, 'invalid_credentials', 'Invalid email or password');
      }
      const token = db.createSession(user.id);
      return sendJson(res, 200, { token, user: publicUser(user) });
    }
    if (pathname === '/api/auth/forgot-password' && method === 'POST') {
      const body = await readBody(req);
      if (typeof body.email !== 'string' || !EMAIL_RE.test(body.email.trim())) {
        return apiError(res, 400, 'validation_error', 'A valid email is required');
      }
      startPasswordReset(body.email);
      return sendJson(res, 202, { message: 'If an account exists, a reset link has been sent.' });
    }

    if (pathname === '/api/products' && method === 'GET') {
      const q = url.searchParams.get('q');
      if (q !== null && flaky && random() < flakyRate) {
        // Simulates an overloaded search backend (planted defect #2).
        return apiError(res, 503, 'search_unavailable', 'Search is temporarily unavailable, please retry');
      }
      const products = db.searchProducts(q);
      return sendJson(res, 200, { products, count: products.length });
    }
    const productMatch = pathname.match(/^\/api\/products\/([^/]+)$/);
    if (productMatch && method === 'GET') {
      const product = db.product(decodeURIComponent(productMatch[1]));
      if (!product) return apiError(res, 404, 'not_found', 'Product not found');
      return sendJson(res, 200, product);
    }

    // Everything below requires an authenticated session (Bearer token or cookie).
    const ctx = getContext(req);
    const user = currentUser(ctx);
    const requireAuth = () => {
      if (!ctx.session || !user) throw new HttpError(401, 'unauthorized', 'Authentication required');
    };

    if (pathname === '/api/cart' && method === 'GET') {
      requireAuth();
      const { lines, totals } = cartView(ctx.session);
      return sendJson(res, 200, { items: lines, subtotal: totals.subtotal, currency: totals.currency });
    }
    if (pathname === '/api/cart/items' && method === 'POST') {
      requireAuth();
      const body = await readBody(req);
      if (typeof body.product_id !== 'string') {
        return apiError(res, 400, 'validation_error', 'product_id is required');
      }
      addToCart(ctx.session, body.product_id, body.quantity ?? 1);
      const { lines, totals } = cartView(ctx.session);
      return sendJson(res, 201, { items: lines, subtotal: totals.subtotal, currency: totals.currency });
    }
    const cartItemMatch = pathname.match(/^\/api\/cart\/items\/([^/]+)$/);
    if (cartItemMatch && method === 'DELETE') {
      requireAuth();
      const id = decodeURIComponent(cartItemMatch[1]);
      if (!ctx.session.cart.delete(id)) return apiError(res, 404, 'not_found', 'Product is not in the cart');
      const { lines, totals } = cartView(ctx.session);
      return sendJson(res, 200, { items: lines, subtotal: totals.subtotal, currency: totals.currency });
    }
    if (pathname === '/api/checkout' && method === 'POST') {
      requireAuth();
      const body = await readBody(req);
      const order = placeOrder(ctx.session, user, {
        name: typeof body.shipping_name === 'string' && body.shipping_name.trim() ? body.shipping_name.trim() : user.name,
        address:
          typeof body.shipping_address === 'string' && body.shipping_address.trim()
            ? body.shipping_address.trim()
            : '1 Demo Street, Springfield',
      });
      const { order_id, subtotal, tax, shipping, total, currency } = orderJson(order);
      return sendJson(res, 201, { order_id, subtotal, tax, shipping, total, currency });
    }
    const orderMatch = pathname.match(/^\/api\/orders\/([^/]+)$/);
    if (orderMatch && method === 'GET') {
      requireAuth();
      const order = db.order(decodeURIComponent(orderMatch[1]));
      if (!order || order.user_id !== user.id) return apiError(res, 404, 'not_found', 'Order not found');
      return sendJson(res, 200, orderJson(order));
    }

    return apiError(res, 404, 'not_found', `No route for ${method} ${pathname}`);
  }

  // ------------------------------------------------------------ page routes

  async function handlePage(req, res, url) {
    const { pathname } = url;
    const method = req.method;
    const ctx = getContext(req, { create: true });
    const session = ctx.session;
    const user = currentUser(ctx);
    const withCookie = ctx.setCookie ? { 'set-cookie': ctx.setCookie } : {};
    const requireLogin = () => {
      if (user) return false;
      redirect(res, `/login?next=${encodeURIComponent(pathname)}`, withCookie);
      return true;
    };

    if (pathname === '/' && method === 'GET') {
      return page(res, ctx, 200, 'Home', views.homePage({ user, featured: db.products().slice(0, 4) }));
    }

    if (pathname === '/register') {
      if (method === 'GET') return page(res, ctx, 200, 'Register', views.registerPage({}));
      if (method === 'POST') {
        const body = await readBody(req);
        const problem = validateRegistration(body);
        if (problem) {
          return page(res, ctx, 400, 'Register', views.registerPage({ error: problem, values: body }));
        }
        const created = db.createUser(body);
        session.user_id = created.id;
        return redirect(res, '/dashboard', withCookie);
      }
    }

    if (pathname === '/login') {
      if (method === 'GET') {
        if (user) return redirect(res, '/dashboard', withCookie);
        return page(res, ctx, 200, 'Log in', views.loginPage({ next: url.searchParams.get('next') }));
      }
      if (method === 'POST') {
        const body = await readBody(req);
        const found = db.userByEmail(body.email);
        if (!found || typeof body.password !== 'string' || !verifyPassword(body.password, found.password_hash)) {
          return page(
            res,
            ctx,
            401,
            'Log in',
            views.loginPage({ error: 'Invalid email or password', email: body.email, next: body.next }),
          );
        }
        session.user_id = found.id;
        return redirect(res, safeNext(body.next) ?? '/dashboard', withCookie);
      }
    }

    if (pathname === '/logout' && (method === 'POST' || method === 'GET')) {
      if (ctx.sid) db.deleteSession(ctx.sid);
      return redirect(res, '/', { 'set-cookie': clearCookie });
    }

    if (pathname === '/forgot-password') {
      if (method === 'GET') return page(res, ctx, 200, 'Forgot password', views.forgotPasswordPage({}));
      if (method === 'POST') {
        const body = await readBody(req);
        if (typeof body.email !== 'string' || !EMAIL_RE.test(body.email.trim())) {
          return page(res, ctx, 400, 'Forgot password', views.forgotPasswordPage({ error: 'A valid email is required' }));
        }
        startPasswordReset(body.email);
        return page(res, ctx, 200, 'Forgot password', views.forgotPasswordPage({ sent: true }));
      }
    }

    if (pathname === '/reset-password') {
      if (method === 'GET') {
        const token = url.searchParams.get('token') ?? '';
        const error = db.peekResetToken(token) ? null : 'This reset link is invalid or has expired.';
        return page(res, ctx, error ? 400 : 200, 'Reset password', views.resetPasswordPage({ token, error }));
      }
      if (method === 'POST') {
        const body = await readBody(req);
        if (typeof body.new_password !== 'string' || body.new_password.length < 8) {
          return page(
            res,
            ctx,
            400,
            'Reset password',
            views.resetPasswordPage({ token: body.token, error: 'Password must be at least 8 characters' }),
          );
        }
        const userId = db.consumeResetToken(body.token);
        if (!userId) {
          return page(res, ctx, 400, 'Reset password', views.resetPasswordPage({ error: 'This reset link is invalid or has expired.' }));
        }
        db.setPassword(userId, body.new_password);
        return page(res, ctx, 200, 'Reset password', views.resetPasswordPage({ done: true }));
      }
    }

    if (pathname === '/dashboard' && method === 'GET') {
      if (requireLogin()) return;
      return page(res, ctx, 200, 'Dashboard', views.dashboardPage({ user, orders: db.ordersForUser(user.id) }));
    }

    if (pathname === '/products' && method === 'GET') {
      const q = url.searchParams.get('q');
      return page(res, ctx, 200, 'Products', views.productsPage({ products: db.searchProducts(q), q }));
    }
    const productMatch = pathname.match(/^\/products\/([^/]+)$/);
    if (productMatch && method === 'GET') {
      const product = db.product(decodeURIComponent(productMatch[1]));
      if (!product) return page(res, ctx, 404, 'Not found', views.notFoundPage());
      return page(res, ctx, 200, product.name, views.productPage({ product }));
    }

    if (pathname === '/cart' && method === 'GET') {
      return page(res, ctx, 200, 'Cart', views.cartPage(cartView(session)));
    }
    if (pathname === '/cart/add' && method === 'POST') {
      const body = await readBody(req);
      try {
        addToCart(session, body.product_id, body.quantity ?? 1);
        const product = db.product(body.product_id);
        session.flash = { kind: 'success', message: `Added ${product.name} to your cart.` };
      } catch (err) {
        if (!(err instanceof HttpError)) throw err;
        session.flash = { kind: 'error', message: err.message };
      }
      return redirect(res, '/cart', withCookie);
    }
    if (pathname === '/cart/update' && method === 'POST') {
      const body = await readBody(req);
      const qty = Number(body.quantity);
      if (session.cart.has(body.product_id)) {
        if (Number.isInteger(qty) && qty >= 1 && qty <= 99) session.cart.set(body.product_id, qty);
        else session.flash = { kind: 'error', message: 'quantity must be an integer between 1 and 99' };
      }
      return redirect(res, '/cart', withCookie);
    }
    if (pathname === '/cart/remove' && method === 'POST') {
      const body = await readBody(req);
      session.cart.delete(body.product_id);
      return redirect(res, '/cart', withCookie);
    }

    if (pathname === '/checkout') {
      if (requireLogin()) return;
      const view = cartView(session);
      if (view.lines.length === 0) return redirect(res, '/cart', withCookie);
      if (method === 'GET') return page(res, ctx, 200, 'Checkout', views.checkoutPage({ user, ...view }));
      if (method === 'POST') {
        const body = await readBody(req);
        const name = String(body.shipping_name ?? '').trim();
        const address = String(body.shipping_address ?? '').trim();
        if (!name || !address) {
          return page(
            res,
            ctx,
            400,
            'Checkout',
            views.checkoutPage({ user, ...view, error: 'Please enter your name and shipping address' }),
          );
        }
        const order = placeOrder(session, user, { name, address });
        return redirect(res, `/orders/${order.id}`, withCookie);
      }
    }

    const orderMatch = pathname.match(/^\/orders\/([^/]+)$/);
    if (orderMatch && method === 'GET') {
      if (requireLogin()) return;
      const order = db.order(decodeURIComponent(orderMatch[1]));
      if (!order || order.user_id !== user.id) return page(res, ctx, 404, 'Not found', views.notFoundPage());
      return page(res, ctx, 200, `Order ${order.id}`, views.orderPage({ order }));
    }

    return page(res, ctx, 404, 'Not found', views.notFoundPage());
  }

  // ------------------------------------------------------------ dispatcher

  return async function handler(req, res) {
    const url = new URL(req.url ?? '/', 'http://localhost');
    const isApi = url.pathname.startsWith('/api/');
    try {
      if (url.pathname === '/openapi.json' && req.method === 'GET') {
        res.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
        return res.end(OPENAPI);
      }
      const asset = STATIC[url.pathname];
      if (asset && req.method === 'GET') {
        res.writeHead(200, { 'content-type': asset.type, 'cache-control': 'public, max-age=300' });
        return res.end(asset.body);
      }
      if (url.pathname === '/favicon.ico') {
        res.writeHead(204);
        return res.end();
      }
      if (isApi) return await handleApi(req, res, url);
      return await handlePage(req, res, url);
    } catch (err) {
      if (err instanceof HttpError) {
        if (isApi) return apiError(res, err.status, err.code, err.message);
        return sendHtml(res, err.status, `<h1>${views.esc(err.message)}</h1>`);
      }
      console.error('[error]', err);
      if (res.headersSent) return res.end();
      if (isApi) return apiError(res, 500, 'internal_error', 'Internal server error');
      return sendHtml(res, 500, '<h1>Internal server error</h1>');
    }
  };
}

export function startServer({ port = Number(process.env.PORT ?? 4000), host = process.env.HOST, ...options } = {}) {
  const server = createServer(createApp(options));
  return new Promise((resolve) => {
    server.listen(port, host, () => resolve(server));
  });
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const server = await startServer();
  const { port } = server.address();
  const flaky = process.env.DEMO_FLAKY !== '0';
  console.log(`Northwind Supply listening on http://localhost:${port}`);
  console.log(`Seed user: ${SEED_USER.email} (password in README)`);
  console.log(`Flaky search (DEMO_FLAKY): ${flaky ? 'on' : 'off'}`);
  const shutdown = () => server.close(() => process.exit(0));
  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);
}
