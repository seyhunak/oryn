// Environment configuration.

import { BROWSER_NAMES, type BrowserName } from './protocol.js';

function intEnv(name: string, fallback: number, min = 1): number {
  const raw = process.env[name];
  if (raw === undefined || raw.trim() === '') return fallback;
  const n = Number(raw);
  if (!Number.isInteger(n) || n < min) {
    throw new Error(`${name} must be an integer >= ${min} (got ${JSON.stringify(raw)})`);
  }
  return n;
}

export interface Config {
  port: number;
  host: string;
  token: string | null;
  runTimeoutMs: number;
  maxConcurrency: number;
  recorderHeadless: boolean;
  /** Engines the runner downloads itself when missing (ORYN_RUNNER_AUTO_INSTALL, default `chromium`; `none` disables). */
  autoInstall: BrowserName[];
}

export function parseAutoInstall(raw: string | undefined): BrowserName[] {
  if (raw === undefined) return ['chromium'];
  const names = raw.split(',').map((s) => s.trim().toLowerCase()).filter((s) => s !== '' && s !== 'none');
  for (const n of names) {
    if (!(BROWSER_NAMES as readonly string[]).includes(n)) {
      throw new Error(`ORYN_RUNNER_AUTO_INSTALL: unknown browser ${JSON.stringify(n)} (use ${BROWSER_NAMES.join(', ')} or none)`);
    }
  }
  return [...new Set(names)] as BrowserName[];
}

export function loadConfig(): Config {
  const token = process.env['RUNNER_TOKEN'];
  return {
    port: intEnv('PORT', 7400, 0),
    host: process.env['HOST'] || '0.0.0.0',
    token: token && token.length > 0 ? token : null,
    runTimeoutMs: intEnv('ORYN_RUN_TIMEOUT_MS', 300_000),
    maxConcurrency: intEnv('MAX_CONCURRENCY', 2),
    recorderHeadless: process.env['ORYN_RECORDER_HEADLESS'] === '1',
    autoInstall: parseAutoInstall(process.env['ORYN_RUNNER_AUTO_INSTALL']),
  };
}

export const MAX_ARTIFACT_BYTES = 25 * 1024 * 1024;
export const MAX_REQUEST_BYTES = 5 * 1024 * 1024;
