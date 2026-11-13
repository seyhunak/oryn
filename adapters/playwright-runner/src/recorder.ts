// Session recorder: opens a headed browser, captures user interactions as
// oryn.browser/v1 steps. Sensitive field values are redacted inside the page
// before they cross the binding, and again on the Node side.

import { randomUUID } from 'node:crypto';
import type { Browser, BrowserContext, Page } from 'playwright';

import { ENGINES } from './browsers.js';
import type { BrowserName } from './protocol.js';
import { SECRET_PLACEHOLDER, SENSITIVE_PATTERN, isSensitiveField, type FieldDescriptor } from './redaction.js';

export const BINDING_NAME = '__orynRecord';
/** A navigation within this window after a user event is attributed to that event and not recorded. */
export const NAVIGATION_GRACE_MS = 1500;
export const RECORDING_IDLE_TIMEOUT_MS = 30 * 60 * 1000;
export const MAX_RECORDED_EVENTS = 2000;

export type RecordedStep =
  | { id: string; action: 'goto'; url: string }
  | { id: string; action: 'click' | 'check' | 'uncheck'; target: string }
  | { id: string; action: 'fill'; target: string; value: string; redacted?: true }
  | { id: string; action: 'select'; target: string; value: string }
  | { id: string; action: 'press'; key: string; target?: string };

type WithoutId<T> = T extends unknown ? Omit<T, 'id'> : never;

export interface RecordingPayload {
  recording_id: string;
  status: 'recording' | 'stopped';
  url: string;
  events: RecordedStep[];
}

export class NoDisplayError extends Error {
  constructor(cause: string) {
    super(`could not launch a headed browser (is a display available?): ${cause}`);
    this.name = 'NoDisplayError';
  }
}

/** Raw event shape posted by the injected script. Validated defensively. */
interface PageEvent {
  kind: 'click' | 'check' | 'uncheck' | 'fill' | 'select' | 'press';
  target: string;
  value?: string;
  key?: string;
  sensitive?: boolean;
  field?: FieldDescriptor;
}

const str = (v: unknown): v is string => typeof v === 'string';

function parsePageEvent(raw: unknown): PageEvent | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const r = raw as Record<string, unknown>;
  const kinds = ['click', 'check', 'uncheck', 'fill', 'select', 'press'] as const;
  const kind = kinds.find((k) => k === r['kind']);
  if (!kind || !str(r['target']) || r['target'].length === 0) return null;
  const event: PageEvent = { kind, target: r['target'].slice(0, 512) };
  if (str(r['value'])) event.value = r['value'];
  if (str(r['key'])) event.key = r['key'];
  if (r['sensitive'] === true) event.sensitive = true;
  const f = r['field'];
  if (typeof f === 'object' && f !== null) {
    const fr = f as Record<string, unknown>;
    event.field = {
      type: str(fr['type']) ? fr['type'] : null,
      name: str(fr['name']) ? fr['name'] : null,
      id: str(fr['id']) ? fr['id'] : null,
      autocomplete: str(fr['autocomplete']) ? fr['autocomplete'] : null,
      label: str(fr['label']) ? fr['label'] : null,
    };
  }
  return event;
}

/** Script injected into every page/frame. Plain ES2017, no dependencies. */
export function recorderScript(): string {
  return `(() => {
  if (window.__orynRecorderInstalled) return;
  window.__orynRecorderInstalled = true;
  const SENSITIVE = new RegExp(${JSON.stringify(SENSITIVE_PATTERN.source)}, ${JSON.stringify(SENSITIVE_PATTERN.flags)});
  const SECRET = ${JSON.stringify(SECRET_PLACEHOLDER)};
  const send = (ev) => { try { window.${BINDING_NAME}(ev); } catch (e) {} };
  const clean = (s) => (s || '').replace(/\\s+/g, ' ').trim();
  const lastFilled = new WeakMap();

  function labelOf(el) {
    if (el.labels && el.labels.length) return clean(el.labels[0].innerText || el.labels[0].textContent);
    const by = el.getAttribute && el.getAttribute('aria-labelledby');
    if (by) {
      const ref = document.getElementById(by.split(/\\s+/)[0]);
      if (ref) return clean(ref.innerText || ref.textContent);
    }
    const wrap = el.closest && el.closest('label');
    if (wrap) return clean(wrap.innerText || wrap.textContent);
    return '';
  }
  function cssPath(el) {
    if (el.id && /^[A-Za-z][\\w-]*$/.test(el.id)) return '#' + el.id;
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && node !== document.body && parts.length < 6) {
      let part = node.tagName.toLowerCase();
      if (node.id && /^[A-Za-z][\\w-]*$/.test(node.id)) { parts.unshift('#' + node.id); break; }
      const parent = node.parentElement;
      if (parent) {
        const same = Array.from(parent.children).filter((c) => c.tagName === node.tagName);
        if (same.length > 1) part += ':nth-of-type(' + (same.indexOf(node) + 1) + ')';
      }
      parts.unshift(part);
      node = parent;
    }
    return 'css=' + parts.join(' > ');
  }
  const isFormField = (el) => /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName);
  function describe(el) {
    const aria = clean(el.getAttribute('aria-label'));
    if (aria) return aria;
    if (isFormField(el)) {
      const label = labelOf(el);
      if (label) return label;
    }
    const tag = el.tagName;
    const role = el.getAttribute('role');
    if (tag === 'BUTTON' || tag === 'A' || role === 'button' || role === 'link') {
      const text = clean(el.innerText || el.textContent);
      if (text && text.length <= 80) return text;
    }
    if (tag === 'INPUT' && /^(submit|button|reset)$/i.test(el.type) && el.value) return clean(el.value);
    const ph = clean(el.getAttribute('placeholder'));
    if (ph) return ph;
    const testid = el.getAttribute('data-testid');
    if (testid) return testid;
    return cssPath(el);
  }
  function field(el) {
    return {
      type: el.getAttribute('type') || el.type || null,
      name: el.getAttribute('name'),
      id: el.id || null,
      autocomplete: el.getAttribute('autocomplete'),
      label: labelOf(el) || clean(el.getAttribute('aria-label')) || null,
    };
  }
  function isSensitive(el) {
    const f = field(el);
    if ((f.type || '').toLowerCase() === 'password') return true;
    return [f.name, f.id, f.autocomplete, f.label].some((v) => typeof v === 'string' && SENSITIVE.test(v));
  }
  const textLike = (el) =>
    el.tagName === 'TEXTAREA' ||
    (el.tagName === 'INPUT' && !/^(checkbox|radio|submit|button|reset|file|image|hidden)$/i.test(el.type));
  const pending = new Set(); // text fields edited but not yet recorded
  let lastEnterAt = 0;
  function recordFill(el) {
    pending.delete(el);
    const value = el.value;
    if (lastFilled.get(el) === value) return;
    lastFilled.set(el, value);
    const sensitive = isSensitive(el);
    send({ kind: 'fill', target: describe(el), value: sensitive ? SECRET : value, sensitive, field: field(el) });
  }
  // Record edits whose change event has not fired yet, so steps keep their real order.
  function flush(except) {
    for (const el of Array.from(pending)) if (el !== except && el.isConnected) recordFill(el);
    pending.clear();
  }

  document.addEventListener('input', (e) => {
    const el = e.target;
    if (el instanceof Element && textLike(el)) pending.add(el);
  }, true);

  document.addEventListener('click', (e) => {
    if (!e.isTrusted) return;
    const raw = e.target instanceof Element ? e.target : null;
    if (!raw) return;
    const el = raw.closest('button, a, input, select, textarea, label, summary, [role=button], [role=link], [role=checkbox], [role=tab], [role=menuitem], [onclick]') || raw;
    if (el.tagName === 'LABEL' && el.control) return; // the click is re-dispatched to the control
    if (el.tagName === 'SELECT' || textLike(el)) return; // recorded via change/fill
    // Keyboard-synthesised click (implicit form submission / Enter on a button) after a recorded Enter.
    if (e.detail === 0 && Date.now() - lastEnterAt < 500) return;
    flush();
    if (el.tagName === 'INPUT' && /^(checkbox|radio)$/i.test(el.type)) {
      send({ kind: el.checked || el.type === 'radio' ? 'check' : 'uncheck', target: describe(el) });
      return;
    }
    send({ kind: 'click', target: describe(el) });
  }, true);

  document.addEventListener('change', (e) => {
    const el = e.target;
    if (!(el instanceof Element)) return;
    if (el.tagName === 'SELECT') { flush(); send({ kind: 'select', target: describe(el), value: el.value }); return; }
    if (textLike(el)) recordFill(el);
  }, true);

  document.addEventListener('keydown', (e) => {
    if (!e.isTrusted || e.key !== 'Enter') return;
    const el = e.target instanceof Element ? e.target : null;
    flush(el);
    if (el && textLike(el) && el.tagName !== 'TEXTAREA') recordFill(el); // change may fire after submit
    lastEnterAt = Date.now();
    const target = el && el !== document.body ? describe(el) : undefined;
    send(target ? { kind: 'press', key: 'Enter', target } : { kind: 'press', key: 'Enter', target: 'css=body' });
  }, true);
})();`;
}

export class Recording {
  readonly id = randomUUID();
  readonly url: string;
  readonly #origin: string;
  readonly #events: RecordedStep[] = [];
  #status: 'recording' | 'stopped' = 'recording';
  #browser: Browser | null = null;
  #lastUserEventAt = 0;
  #seq = 0;
  #idleTimer: NodeJS.Timeout | null = null;
  #onStopped: (() => void) | null = null;

  constructor(url: string) {
    this.url = url;
    this.#origin = new URL(url).origin;
  }

  get status(): 'recording' | 'stopped' {
    return this.#status;
  }

  payload(): RecordingPayload {
    return { recording_id: this.id, status: this.#status, url: this.url, events: [...this.#events] };
  }

  async start(browserName: BrowserName, headless: boolean, onStopped: () => void): Promise<void> {
    this.#onStopped = onStopped;
    try {
      this.#browser = await ENGINES[browserName].launch({ headless });
    } catch (e) {
      throw new NoDisplayError((e instanceof Error ? e.message : String(e)).split('\n')[0] ?? '');
    }
    this.#browser.on('disconnected', () => this.#markStopped());
    const context: BrowserContext = await this.#browser.newContext({ viewport: null });
    await context.exposeBinding(BINDING_NAME, (_source, raw: unknown) => this.#onPageEvent(raw));
    await context.addInitScript({ content: recorderScript() });
    context.on('page', (p) => this.#watchPage(p));
    const page = await context.newPage();
    this.#push({ action: 'goto', url: this.#relative(this.url) });
    this.#lastUserEventAt = Date.now(); // the initial navigation is already recorded
    await page.goto(this.url).catch(() => undefined); // a failing first load is still a valid session
    this.#armIdleTimer();
  }

  async stop(): Promise<RecordingPayload> {
    const browser = this.#browser;
    this.#browser = null;
    if (browser) await browser.close().catch(() => undefined);
    this.#markStopped();
    return this.payload();
  }

  #markStopped(): void {
    if (this.#status === 'stopped') return;
    this.#status = 'stopped';
    if (this.#idleTimer) clearTimeout(this.#idleTimer);
    this.#onStopped?.();
  }

  #armIdleTimer(): void {
    if (this.#idleTimer) clearTimeout(this.#idleTimer);
    this.#idleTimer = setTimeout(() => void this.stop(), RECORDING_IDLE_TIMEOUT_MS);
    this.#idleTimer.unref();
  }

  #watchPage(page: Page): void {
    page.on('framenavigated', (frame) => {
      if (frame !== page.mainFrame() || this.#status !== 'recording') return;
      if (Date.now() - this.#lastUserEventAt < NAVIGATION_GRACE_MS) return; // caused by a click/press
      const url = frame.url();
      if (!/^https?:/i.test(url)) return;
      const last = this.#events.at(-1);
      const rel = this.#relative(url);
      if (last?.action === 'goto' && last.url === rel) return;
      this.#push({ action: 'goto', url: rel });
    });
  }

  #relative(url: string): string {
    try {
      const u = new URL(url);
      return u.origin === this.#origin ? `${u.pathname}${u.search}${u.hash}` : u.href;
    } catch {
      return url;
    }
  }

  #onPageEvent(raw: unknown): void {
    if (this.#status !== 'recording') return;
    const ev = parsePageEvent(raw);
    if (!ev) return;
    this.#lastUserEventAt = Date.now();
    this.#armIdleTimer();
    switch (ev.kind) {
      case 'click':
      case 'check':
      case 'uncheck':
        this.#push({ action: ev.kind, target: ev.target });
        break;
      case 'select':
        this.#push({ action: 'select', target: ev.target, value: ev.value ?? '' });
        break;
      case 'press':
        this.#push(ev.target === 'css=body' ? { action: 'press', key: ev.key ?? 'Enter' } : { action: 'press', key: ev.key ?? 'Enter', target: ev.target });
        break;
      case 'fill': {
        // Defence in depth: re-check on this side too; never keep the raw value of a sensitive field.
        const sensitive = ev.sensitive === true || (ev.field !== undefined && isSensitiveField(ev.field));
        if (sensitive) this.#push({ action: 'fill', target: ev.target, value: SECRET_PLACEHOLDER, redacted: true });
        else this.#push({ action: 'fill', target: ev.target, value: ev.value ?? '' });
        break;
      }
    }
  }

  #push(step: WithoutId<RecordedStep>): void {
    if (this.#events.length >= MAX_RECORDED_EVENTS) return;
    this.#events.push({ ...step, id: `r${++this.#seq}` } as RecordedStep);
  }
}

export class RecordingManager {
  readonly #recordings = new Map<string, Recording>();
  readonly #headless: boolean;

  constructor(headless: boolean) {
    this.#headless = headless;
  }

  async create(url: string, browser: BrowserName): Promise<Recording> {
    const rec = new Recording(url);
    this.#recordings.set(rec.id, rec);
    try {
      await rec.start(browser, this.#headless, () => undefined);
    } catch (e) {
      this.#recordings.delete(rec.id);
      await rec.stop();
      throw e;
    }
    return rec;
  }

  get(id: string): Recording | undefined {
    return this.#recordings.get(id);
  }

  async closeAll(): Promise<void> {
    await Promise.allSettled([...this.#recordings.values()].map((r) => r.stop()));
  }
}
