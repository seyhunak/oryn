// Browser pool: one shared headless Browser per engine, launched lazily and
// relaunched if it disconnects. Availability is probed once at startup.

import { chromium, firefox, webkit, type Browser, type BrowserType } from 'playwright';
import { BROWSER_NAMES, type BrowserName } from './protocol.js';

export const ENGINES: Record<BrowserName, BrowserType> = { chromium, firefox, webkit };

export interface BrowserAvailability {
  name: BrowserName;
  version: string | null;
  available: boolean;
  error?: string;
}

export class BrowserUnavailableError extends Error {
  constructor(name: BrowserName, cause: string) {
    super(`browser "${name}" is not available: ${cause}`);
    this.name = 'BrowserUnavailableError';
  }
}

const firstLine = (e: unknown): string => (e instanceof Error ? e.message : String(e)).split('\n')[0] ?? '';

export class BrowserPool {
  readonly #browsers = new Map<BrowserName, Promise<Browser>>();
  #availability: Promise<BrowserAvailability[]> | null = null;
  #closed = false;

  /** Probe every engine once (launch + close). Cached; never throws. */
  availability(): Promise<BrowserAvailability[]> {
    this.#availability ??= Promise.all(
      BROWSER_NAMES.map(async (name): Promise<BrowserAvailability> => {
        try {
          const browser = await this.get(name);
          return { name, version: browser.version(), available: true };
        } catch (e) {
          return { name, version: null, available: false, error: firstLine(e) };
        }
      }),
    );
    return this.#availability;
  }

  /** Shared browser instance for `name` (launched on first use). */
  get(name: BrowserName): Promise<Browser> {
    if (this.#closed) return Promise.reject(new Error('browser pool is closed'));
    let pending = this.#browsers.get(name);
    if (!pending) {
      pending = ENGINES[name].launch({ headless: true }).then(
        (browser) => {
          browser.on('disconnected', () => {
            if (this.#browsers.get(name) === pending) this.#browsers.delete(name);
          });
          return browser;
        },
        (e: unknown) => {
          this.#browsers.delete(name);
          throw new BrowserUnavailableError(name, firstLine(e));
        },
      );
      this.#browsers.set(name, pending);
    }
    return pending;
  }

  async close(): Promise<void> {
    this.#closed = true;
    const all = [...this.#browsers.values()];
    this.#browsers.clear();
    await Promise.allSettled(all.map(async (p) => (await p).close()));
  }
}
