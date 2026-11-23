// HTTP front-end of the ORYN Playwright runner (oryn.browser/v1).

import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import { timingSafeEqual, createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

import { BrowserPool, BrowserUnavailableError } from './browsers.js';
import { loadConfig, MAX_REQUEST_BYTES, type Config } from './config.js';
import { executeRun } from './executor.js';
import { formatIssues, PROTOCOL, recordingRequestSchema, validateRunRequest, type ApiErrorBody } from './protocol.js';
import { NoDisplayError, RecordingManager } from './recorder.js';
import { Semaphore } from './semaphore.js';

class HttpError extends Error {
  readonly status: number;
  readonly code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

function sendJson(res: ServerResponse, status: number, payload: unknown): void {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'content-length': Buffer.byteLength(body),
    'cache-control': 'no-store',
  });
  res.end(body);
}

const errorBody = (code: string, message: string): ApiErrorBody => ({ error: { code, message } });

async function readJson(req: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of req as AsyncIterable<Buffer>) {
    size += chunk.length;
    if (size > MAX_REQUEST_BYTES) throw new HttpError(413, 'payload_too_large', `request body exceeds ${MAX_REQUEST_BYTES} bytes`);
    chunks.push(chunk);
  }
  const text = Buffer.concat(chunks).toString('utf8');
  if (text.trim() === '') throw new HttpError(400, 'invalid_request', 'request body must be a JSON object');
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw new HttpError(400, 'invalid_request', 'request body is not valid JSON');
  }
}

const digest = (s: string): Buffer => createHash('sha256').update(s).digest();

export function isAuthorized(header: string | undefined, token: string | null): boolean {
  if (token === null) return true;
  if (!header?.startsWith('Bearer ')) return false;
  return timingSafeEqual(digest(header.slice(7).trim()), digest(token));
}

export interface RunnerServer {
  server: Server;
  pool: BrowserPool;
  close(): Promise<void>;
}

export function createRunnerServer(config: Config): RunnerServer {
  const pool = new BrowserPool({ autoInstall: config.autoInstall });
  const recordings = new RecordingManager(config.recorderHeadless);
  const slots = new Semaphore(config.maxConcurrency);

  async function route(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const url = new URL(req.url ?? '/', 'http://runner');
    const path = url.pathname.replace(/\/+$/, '') || '/';
    const method = req.method ?? 'GET';

    if (path === '/v1/health') {
      if (method !== 'GET') throw new HttpError(405, 'method_not_allowed', 'use GET');
      const browsers = await pool.availability();
      return sendJson(res, 200, {
        status: 'ok',
        protocol: PROTOCOL,
        browsers: browsers.map(({ name, version, available }) => ({ name, version, available })),
      });
    }

    if (!isAuthorized(req.headers.authorization, config.token)) {
      throw new HttpError(401, 'unauthorized', 'missing or invalid bearer token');
    }

    if (path === '/v1/runs') {
      if (method !== 'POST') throw new HttpError(405, 'method_not_allowed', 'use POST');
      const validation = validateRunRequest(await readJson(req));
      if (!validation.ok) throw new HttpError(400, validation.code, validation.message);
      const run = validation.value;
      const availability = (await pool.availability()).find((b) => b.name === run.browser);
      if (availability && !availability.available) {
        throw new HttpError(503, 'browser_unavailable', `browser "${run.browser}" is not installed or failed to launch`);
      }
      const response = await slots.run(() => executeRun(run, { pool, runTimeoutMs: config.runTimeoutMs }));
      return sendJson(res, 200, response);
    }

    if (path === '/v1/recordings') {
      if (method !== 'POST') throw new HttpError(405, 'method_not_allowed', 'use POST');
      const parsed = recordingRequestSchema.safeParse(await readJson(req));
      if (!parsed.success) throw new HttpError(400, 'invalid_request', formatIssues(parsed.error));
      const rec = await recordings.create(parsed.data.url, parsed.data.browser);
      return sendJson(res, 201, { recording_id: rec.id });
    }

    const recMatch = path.match(/^\/v1\/recordings\/([^/]+)(\/stop)?$/);
    if (recMatch) {
      const rec = recordings.get(decodeURIComponent(recMatch[1] ?? ''));
      if (!rec) throw new HttpError(404, 'not_found', 'unknown recording_id');
      if (recMatch[2]) {
        if (method !== 'POST') throw new HttpError(405, 'method_not_allowed', 'use POST');
        return sendJson(res, 200, await rec.stop());
      }
      if (method !== 'GET') throw new HttpError(405, 'method_not_allowed', 'use GET');
      return sendJson(res, 200, rec.payload());
    }

    throw new HttpError(404, 'not_found', `no route for ${method} ${url.pathname}`);
  }

  const server = createServer((req, res) => {
    route(req, res).catch((e: unknown) => {
      if (res.headersSent) {
        res.end();
        return;
      }
      if (e instanceof HttpError) return sendJson(res, e.status, errorBody(e.code, e.message));
      if (e instanceof NoDisplayError) return sendJson(res, 501, errorBody('no_display', e.message));
      if (e instanceof BrowserUnavailableError) return sendJson(res, 503, errorBody('browser_unavailable', e.message));
      console.error('[runner] internal error:', e instanceof Error ? e.message : e);
      return sendJson(res, 500, errorBody('internal_error', 'internal error'));
    });
  });
  server.requestTimeout = 0; // runs may legitimately take minutes (bounded by ORYN_RUN_TIMEOUT_MS)
  server.headersTimeout = 60_000;

  return {
    server,
    pool,
    async close() {
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await recordings.closeAll();
      await pool.close();
    },
  };
}

async function main(): Promise<void> {
  const config = loadConfig();
  const runner = createRunnerServer(config);
  void runner.pool.availability().then((browsers) => {
    for (const b of browsers) {
      console.log(`[runner] ${b.name}: ${b.available ? `available (${b.version})` : `unavailable (${b.error ?? 'unknown'})`}`);
    }
  });
  runner.server.listen(config.port, config.host, () => {
    console.log(
      `[runner] ${PROTOCOL} listening on http://${config.host}:${config.port} ` +
        `(max concurrency ${config.maxConcurrency}, run timeout ${config.runTimeoutMs} ms, auth ${config.token ? 'on' : 'off'})`,
    );
  });
  let closing = false;
  const shutdown = (): void => {
    if (closing) return;
    closing = true;
    runner.close().finally(() => process.exit(0));
  };
  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  await main();
}
