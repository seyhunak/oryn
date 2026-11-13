// Executes one oryn.browser/v1 run: a fresh BrowserContext, sequential steps,
// raw results + observations. No pass/fail decisions are made here.

import { mkdtemp, readFile, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { errors, type BrowserContext, type Page, type Request } from 'playwright';

import type { BrowserPool } from './browsers.js';
import { MAX_ARTIFACT_BYTES } from './config.js';
import { networkDecision } from './policy.js';
import {
  MAX_WAIT_STEP_MS,
  OBSERVATION_ACTIONS,
  PROTOCOL,
  type Artifact,
  type ConsoleEntry,
  type ErrorKind,
  type NetworkEntry,
  type Observation,
  type RunRequest,
  type RunResponse,
  type Step,
  type StepError,
  type StepResult,
} from './protocol.js';
import { POLL_INTERVAL_MS, TargetNotFoundError, resolveTarget, sleep, tryResolve } from './targets.js';

export const MAX_CONSOLE_ENTRIES = 1000;
export const MAX_NETWORK_ENTRIES = 1000;

export interface ExecutorOptions {
  pool: BrowserPool;
  runTimeoutMs: number;
}

class BlockedNavigationError extends Error {
  constructor(url: string) {
    super(`navigation to ${url} blocked by policy`);
    this.name = 'BlockedNavigationError';
  }
}

class RunTimeoutError extends Error {
  constructor(ms: number) {
    super(`run timeout of ${ms} ms exceeded`);
    this.name = 'RunTimeoutError';
  }
}

const nowIso = (): string => new Date().toISOString();
const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e)).split('\n')[0] ?? 'error';

export function classifyError(step: Step, e: unknown): StepError {
  const message = messageOf(e);
  let kind: ErrorKind = 'unknown';
  if (e instanceof TargetNotFoundError) kind = 'not_found';
  else if (e instanceof BlockedNavigationError || /ERR_BLOCKED_BY_CLIENT|blocked by policy/i.test(message)) {
    kind = 'blocked';
  } else if (e instanceof RunTimeoutError || e instanceof errors.TimeoutError) kind = 'timeout';
  else if (step.action === 'goto' || /net::ERR_|NS_ERROR_|navigation|Could not connect/i.test(message)) {
    kind = 'navigation';
  }
  return { kind, message };
}

export function sanitizeArtifactName(name: string): string {
  const cleaned = name.replace(/[^A-Za-z0-9._-]+/g, '_').replace(/^\.+/, '').slice(0, 100);
  return cleaned.length > 0 ? cleaned : 'screenshot';
}

/** Collected run output shared between the step loop and event listeners. */
class Collector {
  readonly console: ConsoleEntry[] = [];
  readonly network: NetworkEntry[] = [];
  readonly artifacts: Artifact[] = [];
  readonly #names = new Set<string>();

  log(type: string, text: string): void {
    if (this.console.length < MAX_CONSOLE_ENTRIES) this.console.push({ type, text, timestamp: nowIso() });
  }

  net(entry: NetworkEntry): void {
    if (this.network.length < MAX_NETWORK_ENTRIES) this.network.push(entry);
  }

  uniqueName(base: string, prefix: string): string {
    let name = base;
    if (this.#names.has(name)) name = `${prefix}-${base}`;
    let i = 2;
    while (this.#names.has(name)) name = `${prefix}-${i++}-${base}`;
    this.#names.add(name);
    return name;
  }

  /** Add an artifact; returns false (with a console note) when it exceeds the size cap. */
  add(name: string, kind: Artifact['kind'], contentType: string, data: Buffer): boolean {
    if (data.byteLength > MAX_ARTIFACT_BYTES) {
      this.log('warning', `[oryn] artifact ${name} dropped: ${data.byteLength} bytes exceeds the ${MAX_ARTIFACT_BYTES} byte cap`);
      return false;
    }
    this.artifacts.push({
      name,
      kind,
      content_type: contentType,
      data_base64: data.toString('base64'),
      size_bytes: data.byteLength,
    });
    return true;
  }
}

export async function executeRun(req: RunRequest, opts: ExecutorOptions): Promise<RunResponse> {
  const startedAt = Date.now();
  const deadline = startedAt + opts.runTimeoutMs;
  const browser = await opts.pool.get(req.browser);
  const out = new Collector();
  const results: StepResult[] = req.steps.map((s) => ({
    id: s.id,
    action: s.action,
    status: 'skipped',
    started_at: null,
    duration_ms: 0,
    observation: null,
    error: null,
    screenshot: null,
  }));
  let status: RunResponse['status'] = 'completed';

  const workDir = await mkdtemp(join(tmpdir(), 'oryn-run-'));
  let context: BrowserContext | null = null;
  let page: Page | null = null;

  try {
    context = await browser.newContext({
      viewport: req.viewport,
      extraHTTPHeaders: req.headers,
      ...(req.storage_state ? { storageState: req.storage_state } : {}),
      ...(req.record_video ? { recordVideo: { dir: workDir, size: req.viewport } } : {}),
      serviceWorkers: 'block', // keep every request visible to the network policy
      acceptDownloads: false,
    });

    const blocked = new WeakSet<Request>();
    await context.route('**/*', async (route) => {
      const request = route.request();
      try {
        if (networkDecision(request.url(), req.allowed_hosts) === 'block') {
          blocked.add(request);
          out.net({ url: request.url(), method: request.method(), status: null, failure: 'blocked by policy' });
          await route.abort('blockedbyclient');
        } else {
          await route.continue();
        }
      } catch {
        // The page or context went away while routing; nothing to do.
      }
    });
    context.on('console', (msg) => out.log(msg.type(), msg.text()));
    context.on('weberror', (err) => out.log('pageerror', messageOf(err.error())));
    context.on('requestfailed', (request) => {
      if (blocked.has(request)) return;
      out.net({
        url: request.url(),
        method: request.method(),
        status: null,
        failure: request.failure()?.errorText ?? 'request failed',
      });
    });
    context.on('response', (response) => {
      if (response.status() >= 400) {
        out.net({ url: response.url(), method: response.request().method(), status: response.status(), failure: null });
      }
    });

    if (req.record_trace) await context.tracing.start({ screenshots: true, snapshots: true });

    page = await context.newPage();
    page.setDefaultTimeout(req.step_timeout_ms);
    page.setDefaultNavigationTimeout(req.step_timeout_ms);

    const runTimer = (() => {
      let timer: NodeJS.Timeout | undefined;
      const promise = new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new RunTimeoutError(opts.runTimeoutMs)), Math.max(0, deadline - Date.now()));
      });
      promise.catch(() => undefined);
      return { promise, cancel: () => clearTimeout(timer) };
    })();

    try {
      for (let i = 0; i < req.steps.length; i++) {
        const step = req.steps[i]!;
        const result = results[i]!;
        const remaining = deadline - Date.now();
        if (remaining <= 0) {
          status = 'aborted';
          out.log('warning', `[oryn] run timeout of ${opts.runTimeoutMs} ms exceeded; remaining steps skipped`);
          break;
        }
        const stepTimeout = Math.min(req.step_timeout_ms, remaining);
        const t0 = Date.now();
        result.started_at = new Date(t0).toISOString();

        let failed = false;
        try {
          const work = runStep(page, step, req, stepTimeout, out);
          work.catch(() => undefined); // may reject after the race is decided
          const outcome = await Promise.race([work, runTimer.promise]);
          result.status = 'ok';
          result.observation = outcome.observation;
          result.screenshot = outcome.screenshot;
        } catch (e) {
          failed = true;
          result.status = 'error';
          result.error = classifyError(step, e);
          if (e instanceof RunTimeoutError) status = 'aborted';
        }
        result.duration_ms = Date.now() - t0;

        if (status === 'aborted') {
          out.log('warning', `[oryn] run timeout of ${opts.runTimeoutMs} ms exceeded; remaining steps skipped`);
          break;
        }
        if (failed && req.screenshots !== 'off') {
          result.screenshot = await capture(page, out, `${step.id}-failure.png`, step.id);
        } else if (!failed && req.screenshots === 'every_step' && step.action !== 'screenshot') {
          result.screenshot = await capture(page, out, `${step.id}.png`, step.id);
        }
        if (failed) break; // remaining steps stay "skipped"
      }
    } finally {
      runTimer.cancel();
    }

    if (req.record_trace) {
      const tracePath = join(workDir, 'trace.zip');
      try {
        await context.tracing.stop({ path: tracePath });
        out.add('trace.zip', 'trace', 'application/zip', await readFile(tracePath));
      } catch (e) {
        out.log('warning', `[oryn] could not save trace: ${messageOf(e)}`);
      }
    }
  } finally {
    const video = page?.video() ?? null;
    if (context) await context.close().catch(() => undefined); // finalises the video file
    if (video) {
      try {
        const path = await video.path();
        const size = (await stat(path)).size;
        if (size > MAX_ARTIFACT_BYTES) {
          out.log('warning', `[oryn] artifact video.webm dropped: ${size} bytes exceeds the ${MAX_ARTIFACT_BYTES} byte cap`);
        } else {
          out.add('video.webm', 'video', 'video/webm', await readFile(path));
        }
      } catch (e) {
        out.log('warning', `[oryn] could not save video: ${messageOf(e)}`);
      }
    }
    await rm(workDir, { recursive: true, force: true }).catch(() => undefined);
  }

  return {
    protocol: PROTOCOL,
    run_id: req.run_id,
    status,
    browser: req.browser,
    browser_version: browser.version(),
    started_at: new Date(startedAt).toISOString(),
    duration_ms: Date.now() - startedAt,
    steps: results,
    artifacts: out.artifacts,
    console: out.console,
    network: out.network,
  };
}

async function capture(page: Page, out: Collector, base: string, stepId: string): Promise<string | null> {
  try {
    const data = await page.screenshot({ timeout: 10_000 });
    const name = out.uniqueName(base, stepId);
    return out.add(name, 'screenshot', 'image/png', data) ? name : null;
  } catch (e) {
    out.log('warning', `[oryn] screenshot ${base} failed: ${messageOf(e)}`);
    return null;
  }
}

interface StepOutcome {
  observation: Observation | null;
  screenshot: string | null;
}

const done = (observation: Observation | null = null, screenshot: string | null = null): StepOutcome => ({
  observation,
  screenshot,
});

/** Best-effort wait for a navigation an action may have triggered. */
async function settle(page: Page, deadline: number): Promise<void> {
  const ms = deadline - Date.now();
  if (ms > 0) await page.waitForLoadState('load', { timeout: ms }).catch(() => undefined);
}

async function runStep(page: Page, step: Step, req: RunRequest, timeout: number, out: Collector): Promise<StepOutcome> {
  const deadline = Date.now() + timeout;
  const left = (): number => Math.max(1, deadline - Date.now());

  if (OBSERVATION_ACTIONS.has(step.action)) {
    const waitMs = 'wait_ms' in step && step.wait_ms !== undefined ? Math.min(step.wait_ms, timeout) : timeout;
    return done(await observe(page, step, Date.now() + waitMs));
  }

  switch (step.action) {
    case 'goto': {
      const url = new URL(step.url, req.base_url).href;
      if (networkDecision(url, req.allowed_hosts) === 'block') throw new BlockedNavigationError(url);
      await page.goto(url, { timeout, waitUntil: 'load' });
      return done();
    }
    case 'click': {
      const { locator } = await resolveTarget(page, step.target, timeout);
      await locator.click({ timeout: left() });
      await settle(page, deadline);
      return done();
    }
    case 'fill': {
      const { locator } = await resolveTarget(page, step.target, timeout, 'form');
      await locator.fill(step.value, { timeout: left() });
      return done();
    }
    case 'press': {
      if (step.target !== undefined) {
        const { locator } = await resolveTarget(page, step.target, timeout);
        await locator.press(step.key, { timeout: left() });
      } else {
        await page.keyboard.press(step.key);
      }
      await settle(page, deadline);
      return done();
    }
    case 'select': {
      const { locator } = await resolveTarget(page, step.target, timeout, 'form');
      await locator.selectOption(step.value, { timeout: left() });
      await settle(page, deadline);
      return done();
    }
    case 'check': {
      const { locator } = await resolveTarget(page, step.target, timeout, 'form');
      await locator.check({ timeout: left() });
      return done();
    }
    case 'uncheck': {
      const { locator } = await resolveTarget(page, step.target, timeout, 'form');
      await locator.uncheck({ timeout: left() });
      return done();
    }
    case 'hover': {
      const { locator } = await resolveTarget(page, step.target, timeout);
      await locator.hover({ timeout: left() });
      return done();
    }
    case 'wait': {
      await sleep(Math.min(step.ms, MAX_WAIT_STEP_MS));
      return done();
    }
    case 'screenshot': {
      const data = await page.screenshot({ timeout });
      const name = out.uniqueName(`${sanitizeArtifactName(step.name)}.png`, step.id);
      return done(null, out.add(name, 'screenshot', 'image/png', data) ? name : null);
    }
    default:
      throw new Error(`unhandled action ${step.action}`);
  }
}

// ------------------------------------------------------------------ observations
// Observations poll every POLL_INTERVAL_MS until their wait hint holds or the
// deadline passes, then report what they saw. They never fail on "not found".

async function poll<T>(deadline: number, read: () => Promise<T>, satisfied: (v: T) => boolean): Promise<T> {
  for (;;) {
    const value = await read();
    if (satisfied(value) || Date.now() >= deadline) return value;
    await sleep(Math.min(POLL_INTERVAL_MS, Math.max(0, deadline - Date.now())));
  }
}

async function observe(page: Page, step: Step, deadline: number): Promise<Observation> {
  switch (step.action) {
    case 'observe_text': {
      const wantVisible = step.visible ?? true;
      const visibleMatches = page.getByText(step.text).filter({ visible: true });
      return poll(
        deadline,
        async () => {
          const count = await visibleMatches.count().catch(() => 0);
          return { visible: count > 0, count };
        },
        (o) => o.visible === wantVisible,
      );
    }
    case 'observe_element': {
      const contains = step.contains;
      return poll(
        deadline,
        async () => {
          const resolved = await tryResolve(page, step.target);
          if (!resolved) return { visible: false, count: 0, text: null, value: null };
          const visible = await resolved.locator.isVisible().catch(() => false);
          const details = await resolved.locator
            .evaluate(
              (el) => ({
                text: el instanceof HTMLElement ? el.innerText : (el.textContent ?? ''),
                value:
                  el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement
                    ? el.value
                    : null,
              }),
              undefined,
              { timeout: 1_000 },
            )
            .catch(() => ({ text: null, value: null }));
          return {
            visible,
            count: resolved.count,
            text: details.text === null ? null : details.text.trim(),
            value: details.value,
          };
        },
        (o) =>
          o.visible &&
          (contains === undefined || (o.text?.includes(contains) ?? false) || (o.value?.includes(contains) ?? false)),
      );
    }
    case 'observe_url': {
      const contains = step.contains;
      return poll(
        deadline,
        async () => {
          const url = page.url();
          let path = '';
          try {
            path = new URL(url).pathname;
          } catch {
            path = '';
          }
          return { url, path };
        },
        (o) => contains === undefined || o.url.includes(contains),
      );
    }
    case 'observe_title': {
      const equals = step.equals;
      return poll(
        deadline,
        async () => ({ title: await page.title().catch(() => '') }),
        (o) => equals === undefined || o.title === equals,
      );
    }
    default:
      throw new Error(`not an observation: ${step.action}`);
  }
}
