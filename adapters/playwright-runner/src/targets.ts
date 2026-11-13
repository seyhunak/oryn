// Human-friendly target resolution.
//
// `targetStrategies` is pure (unit tested): it turns a target string into an
// ordered list of strategies. `resolveTarget` applies them to a page, polling
// until one of them matches at least one element or the deadline passes.

import type { Locator, Page } from 'playwright';

export type Strategy =
  | { kind: 'css'; selector: string }
  | { kind: 'xpath'; selector: string }
  | { kind: 'text'; text: string; exact: boolean }
  | { kind: 'role'; role: 'button' | 'link'; name: string; exact: boolean }
  | { kind: 'label'; text: string }
  | { kind: 'placeholder'; text: string }
  | { kind: 'testid'; id: string };

/**
 * `interact` (click, hover, press, observe_element) uses the protocol's default order.
 * `form` (fill, select, check, uncheck) needs a form control, so label and placeholder
 * are tried first; otherwise "Password" would match a "Forgot your password?" link.
 */
export type TargetIntent = 'interact' | 'form';

export function targetStrategies(target: string, intent: TargetIntent = 'interact'): Strategy[] {
  if (target.startsWith('css=')) return [{ kind: 'css', selector: target.slice(4) }];
  if (target.startsWith('xpath=')) return [{ kind: 'xpath', selector: target.slice(6) }];
  if (target.startsWith('text=')) return [{ kind: 'text', text: target.slice(5), exact: true }];
  if (/^[#.[]/.test(target) || target.includes('>>')) return [{ kind: 'css', selector: target }];
  // Exact accessible names win over partial ones, so "Cordless Drill 18V" clicks the
  // product link rather than an "Add Cordless Drill 18V to cart" button.
  const buttonExact: Strategy = { kind: 'role', role: 'button', name: target, exact: true };
  const linkExact: Strategy = { kind: 'role', role: 'link', name: target, exact: true };
  const button: Strategy = { kind: 'role', role: 'button', name: target, exact: false };
  const link: Strategy = { kind: 'role', role: 'link', name: target, exact: false };
  const label: Strategy = { kind: 'label', text: target };
  const placeholder: Strategy = { kind: 'placeholder', text: target };
  const testid: Strategy = { kind: 'testid', id: target };
  const text: Strategy = { kind: 'text', text: target, exact: false };
  return intent === 'form'
    ? [label, placeholder, testid, button, link, text]
    : [buttonExact, linkExact, button, link, label, placeholder, testid, text];
}

export function strategyLocator(page: Page, s: Strategy): Locator {
  switch (s.kind) {
    case 'css':
      return page.locator(s.selector);
    case 'xpath':
      return page.locator(`xpath=${s.selector}`);
    case 'text':
      return page.getByText(s.text, { exact: s.exact });
    case 'role':
      return page.getByRole(s.role, { name: s.name, exact: s.exact });
    case 'label':
      return page.getByLabel(s.text);
    case 'placeholder':
      return page.getByPlaceholder(s.text);
    case 'testid':
      return page.getByTestId(s.id);
  }
}

export class TargetNotFoundError extends Error {
  constructor(target: string, timeoutMs: number) {
    super(`no element matched target ${JSON.stringify(target)} within ${timeoutMs} ms`);
    this.name = 'TargetNotFoundError';
  }
}

export const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

export const POLL_INTERVAL_MS = 100;

/** Count matches without ever throwing (invalid selectors count as zero). */
async function safeCount(locator: Locator): Promise<number> {
  try {
    return await locator.count();
  } catch {
    return 0;
  }
}

export interface Resolved {
  locator: Locator; // `.first()` of the winning strategy
  count: number; // total matches of the winning strategy
  strategy: Strategy;
}

/** One pass over the strategies; returns the first with count > 0. */
export async function tryResolve(
  page: Page,
  target: string,
  intent: TargetIntent = 'interact',
): Promise<Resolved | null> {
  for (const strategy of targetStrategies(target, intent)) {
    const all = strategyLocator(page, strategy);
    const count = await safeCount(all);
    if (count > 0) return { locator: all.first(), count, strategy };
  }
  return null;
}

/** Poll until a strategy matches or `timeoutMs` elapses; throws TargetNotFoundError. */
export async function resolveTarget(
  page: Page,
  target: string,
  timeoutMs: number,
  intent: TargetIntent = 'interact',
): Promise<Resolved> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const resolved = await tryResolve(page, target, intent);
    if (resolved) return resolved;
    if (Date.now() >= deadline) throw new TargetNotFoundError(target, timeoutMs);
    await sleep(Math.min(POLL_INTERVAL_MS, Math.max(0, deadline - Date.now())));
  }
}
