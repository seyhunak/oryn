// Environment configuration.

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
  };
}

export const MAX_ARTIFACT_BYTES = 25 * 1024 * 1024;
export const MAX_REQUEST_BYTES = 5 * 1024 * 1024;
