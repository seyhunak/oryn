// Browser pool: one shared headless Browser per engine, launched lazily and
// relaunched if it disconnects.
//
// The runner provisions its own browsers: when an engine listed in
// `autoInstall` fails to launch because its executable is missing (a fresh
// machine, or a Playwright upgrade that wants a newer build), the pool runs
// `playwright install <engine>` once and launches again. While that runs the
// engine reports itself unavailable, so health checks never block on a download.

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
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

export type Installer = (name: BrowserName) => Promise<void>;

export interface BrowserPoolOptions {
  /** Engines the pool may download itself when their executable is missing. */
  autoInstall?: readonly BrowserName[];
  /** How to install an engine (defaults to Playwright's own CLI). */
  install?: Installer;
  /** Override launching (tests). */
  launch?: (name: BrowserName) => Promise<Browser>;
}

const firstLine = (e: unknown): string => (e instanceof Error ? e.message : String(e)).split('\n')[0] ?? '';

/** Playwright's error when the browser build for this Playwright version is not downloaded. */
export const isMissingExecutable = (e: unknown): boolean =>
  /Executable doesn't exist|please run the following command to download new browsers/i.test(
    e instanceof Error ? e.message : String(e),
  );

/** Install one engine with the Playwright CLI that this runner depends on (same version, same cache). */
export const playwrightInstall: Installer = (name) =>
  new Promise((resolve, reject) => {
    // `playwright/cli` is not an exported subpath, so locate cli.js next to the package manifest.
    const cli = join(dirname(createRequire(import.meta.url).resolve('playwright/package.json')), 'cli.js');
    console.log(`[runner] ${name}: executable missing, installing it (playwright install ${name})`);
    const child = spawn(process.execPath, [cli, 'install', name], { stdio: ['ignore', 'inherit', 'inherit'] });
    child.on('error', reject);
    child.on('exit', (code) =>
      code === 0 ? resolve() : reject(new Error(`playwright install ${name} exited with code ${code}`)),
    );
  });

export class BrowserPool {
  readonly #browsers = new Map<BrowserName, Promise<Browser>>();
  readonly #installs = new Map<BrowserName, Promise<void>>();
  readonly #installFailed = new Map<BrowserName, string>();
  readonly #autoInstall: ReadonlySet<BrowserName>;
  readonly #install: Installer;
  readonly #launch: (name: BrowserName) => Promise<Browser>;
  #availability: Promise<BrowserAvailability[]> | null = null;
  #closed = false;

  constructor(options: BrowserPoolOptions = {}) {
    this.#autoInstall = new Set(options.autoInstall ?? []);
    this.#install = options.install ?? playwrightInstall;
    this.#launch = options.launch ?? ((name) => ENGINES[name].launch({ headless: true }));
  }

  /**
   * Probe every engine (launch + close). Never throws. The result is cached
   * once every engine is either available or definitively unavailable; while
   * an install is running the engine reports `installing` and the next call
   * probes again.
   */
  availability(): Promise<BrowserAvailability[]> {
    if (this.#availability) return this.#availability;
    const probe = Promise.all(
      BROWSER_NAMES.map(async (name): Promise<BrowserAvailability> => {
        try {
          const browser = await this.#getOrStartInstall(name);
          return { name, version: browser.version(), available: true };
        } catch (e) {
          return { name, version: null, available: false, error: firstLine(e) };
        }
      }),
    );
    this.#availability = probe;
    void probe.then(() => {
      if (this.#installs.size > 0 && this.#availability === probe) this.#availability = null;
    });
    return probe;
  }

  /** Shared browser instance for `name`; waits for an install of it if one is running or needed. */
  async get(name: BrowserName): Promise<Browser> {
    let install = this.#installs.get(name);
    if (!install) {
      try {
        return await this.#launchShared(name);
      } catch (e) {
        if (!this.#mayInstall(name, e)) throw e;
        install = this.#startInstall(name);
      }
    }
    await install.catch(() => undefined);
    return this.#launchShared(name);
  }

  /** Launch, or - when the executable is missing and the engine may be installed - start installing and fail fast. */
  async #getOrStartInstall(name: BrowserName): Promise<Browser> {
    if (this.#installs.has(name)) throw new BrowserUnavailableError(name, 'installing');
    try {
      return await this.#launchShared(name);
    } catch (e) {
      if (!this.#mayInstall(name, e)) throw e;
      this.#startInstall(name);
      throw new BrowserUnavailableError(name, 'installing');
    }
  }

  #mayInstall(name: BrowserName, launchError: unknown): boolean {
    return isMissingExecutable(launchError) && this.#autoInstall.has(name) && !this.#installFailed.has(name);
  }

  #startInstall(name: BrowserName): Promise<void> {
    const running = this.#installs.get(name);
    if (running) return running;
    const install = this.#install(name).then(
      () => {
        this.#installs.delete(name);
        this.#availability = null;
        console.log(`[runner] ${name}: installed`);
      },
      (e: unknown) => {
        this.#installs.delete(name);
        this.#installFailed.set(name, firstLine(e));
        this.#availability = null;
        console.error(`[runner] ${name}: install failed (${firstLine(e)})`);
        throw e;
      },
    );
    install.catch(() => undefined);
    this.#installs.set(name, install);
    return install;
  }

  #launchShared(name: BrowserName): Promise<Browser> {
    if (this.#closed) return Promise.reject(new Error('browser pool is closed'));
    let pending = this.#browsers.get(name);
    if (!pending) {
      pending = this.#launch(name).then(
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
