// In-memory data store. Everything is lost on restart; POST /api/__reset
// restores the seed state without restarting the process.

import { randomBytes, scryptSync, timingSafeEqual } from 'node:crypto';

export const SEED_USER = Object.freeze({
  name: 'Demo Customer',
  email: 'demo@northwind.test',
  password: 'demo-password',
});

const PRODUCTS = [
  {
    id: 'claw-hammer',
    sku: 'NW-1001',
    name: 'Claw Hammer',
    category: 'Hand tools',
    price: 12.5,
    stock: 140,
    description: '16 oz forged steel hammer with a fibreglass handle and anti-vibration grip.',
  },
  {
    id: 'tape-measure',
    sku: 'NW-1002',
    name: 'Tape Measure 25 ft',
    category: 'Measuring',
    price: 9.99,
    stock: 220,
    description: 'Rugged 25 ft tape with a magnetic hook and belt clip.',
  },
  {
    id: 'cordless-drill',
    sku: 'NW-2001',
    name: 'Cordless Drill 18V',
    category: 'Power tools',
    price: 89.0,
    stock: 35,
    description: '18V drill/driver with two batteries, charger and carry case.',
  },
  {
    id: 'safety-glasses',
    sku: 'NW-3001',
    name: 'Safety Glasses',
    category: 'Safety',
    price: 6.25,
    stock: 400,
    description: 'Anti-fog, scratch resistant safety glasses. ANSI Z87.1 rated.',
  },
  {
    id: 'work-gloves',
    sku: 'NW-3002',
    name: 'Leather Work Gloves',
    category: 'Safety',
    price: 14.75,
    stock: 180,
    description: 'Cowhide gloves with a reinforced palm. One size fits most.',
  },
  {
    id: 'socket-set',
    sku: 'NW-1003',
    name: 'Socket Set (40 pc)',
    category: 'Hand tools',
    price: 54.0,
    stock: 60,
    description: 'Metric and SAE sockets, ratchet and extensions in a blow-moulded case.',
  },
  {
    id: 'wood-screws',
    sku: 'NW-4001',
    name: 'Wood Screws (box of 200)',
    category: 'Fasteners',
    price: 7.4,
    stock: 900,
    description: 'Assorted zinc-plated countersunk wood screws.',
  },
  {
    id: 'step-ladder',
    sku: 'NW-5001',
    name: 'Aluminium Step Ladder',
    category: 'Ladders',
    price: 119.0,
    stock: 20,
    description: 'Four-step folding ladder with a tool tray. Rated to 150 kg.',
  },
];

export function hashPassword(password) {
  const salt = randomBytes(16);
  const hash = scryptSync(password, salt, 32);
  return `${salt.toString('hex')}:${hash.toString('hex')}`;
}

export function verifyPassword(password, stored) {
  const [saltHex, hashHex] = stored.split(':');
  const expected = Buffer.from(hashHex, 'hex');
  const actual = scryptSync(password, Buffer.from(saltHex, 'hex'), expected.length);
  return timingSafeEqual(expected, actual);
}

export const newId = (bytes = 16) => randomBytes(bytes).toString('hex');

// Hashing the seed password is comparatively slow; do it once per process.
const SEED_PASSWORD_HASH = hashPassword(SEED_USER.password);

function seed() {
  const users = new Map();
  const seedUser = {
    id: 'u-1',
    name: SEED_USER.name,
    email: SEED_USER.email,
    password_hash: SEED_PASSWORD_HASH,
    created_at: new Date().toISOString(),
  };
  users.set(seedUser.id, seedUser);
  return {
    users, // id -> user
    sessions: new Map(), // session id / API token -> { user_id | null, cart: Map<product_id, qty>, flash }
    orders: new Map(), // order id -> order
    resetTokens: new Map(), // token -> { user_id, expires_at }
    outbox: [], // "sent" e-mails, exposed at GET /api/__outbox for testing
    nextUserNo: 2,
    nextOrderNo: 1001,
  };
}

let state = seed();

export const db = {
  /**
   * Remove everything tests created (accounts other than the seeded customer, their
   * sessions and orders, reset tokens, sent e-mails) while keeping live sessions of the
   * seeded customer. Suites run concurrently on several workers, so a reset issued by one
   * scenario must not sign out, or empty the session-scoped cart of, another.
   */
  reset() {
    const fresh = seed();
    for (const [id, session] of state.sessions) {
      if (session.user_id === null || fresh.users.has(session.user_id)) fresh.sessions.set(id, session);
    }
    for (const [id, order] of state.orders) {
      if (fresh.users.has(order.user_id)) fresh.orders.set(id, order);
    }
    fresh.nextOrderNo = state.nextOrderNo;
    state = fresh;
  },
  products() {
    return PRODUCTS;
  },
  product(id) {
    return PRODUCTS.find((p) => p.id === id) ?? null;
  },
  searchProducts(q) {
    const needle = (q ?? '').trim().toLowerCase();
    if (!needle) return PRODUCTS;
    return PRODUCTS.filter((p) =>
      [p.name, p.category, p.description, p.sku].some((f) => f.toLowerCase().includes(needle)),
    );
  },
  userByEmail(email) {
    const e = String(email ?? '').trim().toLowerCase();
    for (const u of state.users.values()) if (u.email === e) return u;
    return null;
  },
  user(id) {
    return state.users.get(id) ?? null;
  },
  createUser({ name, email, password }) {
    const user = {
      id: `u-${state.nextUserNo++}`,
      name: name.trim(),
      email: email.trim().toLowerCase(),
      password_hash: hashPassword(password),
      created_at: new Date().toISOString(),
    };
    state.users.set(user.id, user);
    return user;
  },
  setPassword(userId, password) {
    const u = state.users.get(userId);
    if (u) u.password_hash = hashPassword(password);
  },
  // Sessions double as API bearer tokens.
  createSession(userId = null) {
    const id = newId(24);
    state.sessions.set(id, { user_id: userId, cart: new Map(), flash: null });
    return id;
  },
  session(id) {
    return id ? (state.sessions.get(id) ?? null) : null;
  },
  deleteSession(id) {
    state.sessions.delete(id);
  },
  createOrder(order) {
    const id = `NW-${state.nextOrderNo++}`;
    const full = { id, created_at: new Date().toISOString(), ...order };
    state.orders.set(id, full);
    return full;
  },
  order(id) {
    return state.orders.get(id) ?? null;
  },
  ordersForUser(userId) {
    return [...state.orders.values()].filter((o) => o.user_id === userId).reverse();
  },
  createResetToken(userId) {
    const token = newId(20);
    state.resetTokens.set(token, { user_id: userId, expires_at: Date.now() + 30 * 60 * 1000 });
    return token;
  },
  consumeResetToken(token) {
    const entry = state.resetTokens.get(token);
    if (!entry || entry.expires_at < Date.now()) return null;
    state.resetTokens.delete(token);
    return entry.user_id;
  },
  peekResetToken(token) {
    const entry = state.resetTokens.get(token);
    return Boolean(entry && entry.expires_at >= Date.now());
  },
  sendMail(mail) {
    state.outbox.push({ ...mail, sent_at: new Date().toISOString() });
  },
  outbox() {
    return state.outbox;
  },
};

/** Expand a session cart into priced line items. */
export function cartLines(session) {
  const lines = [];
  for (const [productId, quantity] of session.cart) {
    const p = db.product(productId);
    if (!p) continue;
    lines.push({
      product_id: p.id,
      name: p.name,
      unit_price: p.price,
      quantity,
      line_total: Math.round(p.price * 100 * quantity) / 100,
    });
  }
  return lines;
}

export const publicUser = (u) => ({ id: u.id, name: u.name, email: u.email });
