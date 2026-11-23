// Drives a real headless Chromium against a tiny local static site.
// Skips (does not fail) when Chromium is not installed.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { chromium } from 'playwright';

import { createRunnerServer, type RunnerServer } from '../src/server.js';
import { PROTOCOL, type RunResponse } from '../src/protocol.js';
import { recorderScript, BINDING_NAME } from '../src/recorder.js';

const chromiumInstalled = (() => {
  try {
    return existsSync(chromium.executablePath());
  } catch {
    return false;
  }
})();
const skip = chromiumInstalled ? false : 'chromium is not installed (run `npx playwright install chromium`)';

const PAGES: Record<string, string> = {
  '/login.html': `<!doctype html><html><head><title>Sign in</title></head><body>
    <h1>Sign in</h1>
    <form action="/welcome.html" method="get">
      <label for="email">Email</label><input id="email" name="email" type="email">
      <label for="password">Password</label><input id="password" name="password" type="password">
      <label for="color">Favourite colour</label>
      <select id="color" name="color"><option value="red">Red</option><option value="blue">Blue</option></select>
      <label><input type="checkbox" name="remember"> Remember me</label>
      <input name="q" placeholder="Search everything">
      <button type="submit">Sign in</button>
    </form>
    <p><a href="/forgot.html">Forgot your password?</a></p>
    <p hidden>Hidden secret text</p>
  </body></html>`,
  '/welcome.html': `<!doctype html><html><head><title>Welcome</title></head><body>
    <h1>Welcome back</h1>
    <img src="http://169.254.169.254/latest/meta-data/x.png" alt="">
    <img src="http://blocked.example.test/pixel.png" alt="">
    <script>console.log('welcome page loaded'); fetch('/missing.json');</script>
  </body></html>`,
};

let site: Server;
let siteUrl: string;
let runner: RunnerServer;
let runnerUrl: string;
const TOKEN = 'test-token';

const listen = (server: Server): Promise<number> =>
  new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve((server.address() as AddressInfo).port)));

before(async () => {
  site = createServer((req, res) => {
    const path = new URL(req.url ?? '/', 'http://x').pathname;
    const body = PAGES[path];
    if (!body) {
      res.writeHead(404, { 'content-type': 'text/plain' });
      res.end('not found');
      return;
    }
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    res.end(body);
  });
  siteUrl = `http://127.0.0.1:${await listen(site)}`;
  runner = createRunnerServer({
    port: 0,
    host: '127.0.0.1',
    token: TOKEN,
    runTimeoutMs: 60_000,
    maxConcurrency: 2,
    recorderHeadless: true,
    autoInstall: [],
  });
  runnerUrl = `http://127.0.0.1:${await listen(runner.server)}`;
});

after(async () => {
  await runner?.close();
  await new Promise<void>((resolve) => site.close(() => resolve()));
});

const post = async (path: string, body: unknown, token: string | null = TOKEN) => {
  const headers: Record<string, string> = { 'content-type': 'application/json' };
  if (token) headers['authorization'] = `Bearer ${token}`;
  const res = await fetch(`${runnerUrl}${path}`, { method: 'POST', headers, body: JSON.stringify(body) });
  return { status: res.status, json: (await res.json()) as unknown };
};

const runRequest = (steps: unknown[], extra: Record<string, unknown> = {}) => ({
  protocol: PROTOCOL,
  run_id: '00000000-0000-4000-8000-000000000001',
  browser: 'chromium',
  base_url: siteUrl,
  step_timeout_ms: 3000,
  screenshots: 'on_failure',
  allowed_hosts: ['127.0.0.1'],
  steps,
  ...extra,
});

test('health is public and reports chromium', { skip }, async () => {
  const res = await fetch(`${runnerUrl}/v1/health`);
  assert.equal(res.status, 200);
  const body = (await res.json()) as { status: string; protocol: string; browsers: Array<{ name: string; available: boolean; version: string }> };
  assert.equal(body.status, 'ok');
  assert.equal(body.protocol, PROTOCOL);
  const c = body.browsers.find((b) => b.name === 'chromium');
  assert.equal(c?.available, true);
  assert.match(c?.version ?? '', /^\d+\./);
  assert.deepEqual(body.browsers.map((b) => b.name), ['chromium', 'firefox', 'webkit']);
});

test('auth and validation errors', { skip }, async () => {
  const unauth = await post('/v1/runs', runRequest([{ id: 's1', action: 'goto', url: '/' }]), null);
  assert.equal(unauth.status, 401);
  const bad = await post('/v1/runs', { ...runRequest([]), steps: [] });
  assert.equal(bad.status, 400);
  assert.equal((bad.json as { error: { code: string } }).error.code, 'invalid_request');
  const proto = await post('/v1/runs', { ...runRequest([{ id: 's1', action: 'goto', url: '/' }]), protocol: 'oryn.browser/v0' });
  assert.equal(proto.status, 400);
  assert.equal((proto.json as { error: { code: string } }).error.code, 'unsupported_protocol');
});

test('end-to-end run in chromium: actions, observations, failure handling', { skip }, async () => {
  const { status, json } = await post(
    '/v1/runs',
    runRequest([
      { id: 's1', action: 'goto', url: '/login.html' },
      { id: 's2', action: 'fill', target: 'Email', value: 'demo@example.test' },
      { id: 's3', action: 'fill', target: 'Password', value: 'hunter2' },
      { id: 's3b', action: 'select', target: 'Favourite colour', value: 'blue' },
      { id: 's3c', action: 'check', target: 'Remember me' },
      { id: 's3d', action: 'fill', target: 'Search everything', value: 'hammer' },
      { id: 's3e', action: 'observe_element', target: 'Email', contains: 'demo@' },
      { id: 's3f', action: 'observe_text', text: 'Hidden secret text', wait_ms: 200 },
      { id: 's4', action: 'click', target: 'Sign in' },
      { id: 's5', action: 'observe_text', text: 'Welcome back' },
      { id: 's6', action: 'observe_text', text: 'This text never appears', wait_ms: 300 },
      { id: 's6b', action: 'observe_text', text: 'Welcome back', visible: false, wait_ms: 300 },
      { id: 's7', action: 'observe_url', contains: '/welcome' },
      { id: 's8', action: 'observe_title', equals: 'Welcome' },
      { id: 's8b', action: 'observe_element', target: 'css=h1', contains: 'Welcome' },
      { id: 's8c', action: 'observe_element', target: '#does-not-exist', wait_ms: 200 },
      { id: 's8d', action: 'screenshot', name: 'welcome page' },
      { id: 's9', action: 'click', target: 'A button that does not exist' },
      { id: 's10', action: 'observe_title' },
      { id: 's11', action: 'goto', url: '/login.html' },
    ]),
  );
  assert.equal(status, 200, JSON.stringify(json));
  const run = json as RunResponse;
  const byId = Object.fromEntries(run.steps.map((s) => [s.id, s]));
  const ok = (id: string) => assert.equal(byId[id]?.status, 'ok', `${id}: ${JSON.stringify(byId[id])}`);

  assert.equal(run.protocol, PROTOCOL);
  assert.equal(run.status, 'completed');
  assert.equal(run.browser, 'chromium');
  assert.match(run.browser_version, /^\d+\./);
  for (const id of ['s1', 's2', 's3', 's3b', 's3c', 's3d', 's3e', 's3f', 's4', 's5', 's6', 's6b', 's7', 's8', 's8b', 's8c', 's8d']) ok(id);

  assert.deepEqual(byId['s3e']?.observation, { visible: true, count: 1, text: '', value: 'demo@example.test' });
  assert.deepEqual(byId['s3f']?.observation, { visible: false, count: 0 });
  assert.deepEqual(byId['s5']?.observation, { visible: true, count: 1 });
  assert.deepEqual(byId['s6']?.observation, { visible: false, count: 0 });
  assert.deepEqual(byId['s6b']?.observation, { visible: true, count: 1 });
  const url = byId['s7']?.observation as { url: string; path: string };
  assert.equal(url.path, '/welcome.html');
  assert.match(url.url, /email=demo%40example\.test/);
  assert.match(url.url, /color=blue/);
  assert.match(url.url, /remember=on/);
  assert.deepEqual(byId['s8']?.observation, { title: 'Welcome' });
  assert.deepEqual(byId['s8b']?.observation, { visible: true, count: 1, text: 'Welcome back', value: null });
  assert.deepEqual(byId['s8c']?.observation, { visible: false, count: 0, text: null, value: null });
  assert.equal(byId['s8d']?.screenshot, 'welcome_page.png');

  assert.equal(byId['s9']?.status, 'error');
  assert.equal(byId['s9']?.error?.kind, 'not_found');
  assert.equal(byId['s9']?.screenshot, 's9-failure.png');
  assert.equal(byId['s10']?.status, 'skipped');
  assert.equal(byId['s10']?.started_at, null);
  assert.equal(byId['s11']?.status, 'skipped');

  const names = run.artifacts.map((a) => a.name).sort();
  assert.deepEqual(names, ['s9-failure.png', 'welcome_page.png']);
  for (const a of run.artifacts) {
    assert.equal(a.kind, 'screenshot');
    assert.equal(a.content_type, 'image/png');
    assert.equal(Buffer.from(a.data_base64, 'base64').subarray(1, 4).toString(), 'PNG');
    assert.equal(Buffer.from(a.data_base64, 'base64').byteLength, a.size_bytes);
  }

  assert.ok(run.console.some((c) => c.type === 'log' && c.text === 'welcome page loaded'), JSON.stringify(run.console));
  const blocked = run.network.filter((n) => n.failure === 'blocked by policy').map((n) => new URL(n.url).hostname);
  assert.ok(blocked.includes('169.254.169.254'), JSON.stringify(run.network));
  assert.ok(blocked.includes('blocked.example.test'), JSON.stringify(run.network));
  assert.ok(run.network.some((n) => n.status === 404 && n.url.endsWith('/missing.json')), JSON.stringify(run.network));
});

test('navigation to a host outside allowed_hosts is a blocked error', { skip }, async () => {
  const { json } = await post(
    '/v1/runs',
    runRequest([
      { id: 's1', action: 'goto', url: 'http://elsewhere.example.test/' },
      { id: 's2', action: 'observe_url' },
    ], { screenshots: 'off' }),
  );
  const run = json as RunResponse;
  assert.equal(run.steps[0]?.status, 'error');
  assert.equal(run.steps[0]?.error?.kind, 'blocked');
  assert.equal(run.steps[0]?.screenshot, null);
  assert.equal(run.steps[1]?.status, 'skipped');
  assert.equal(run.artifacts.length, 0);
});

test('every_step screenshots, video and trace artifacts', { skip }, async () => {
  const { json } = await post(
    '/v1/runs',
    runRequest(
      [
        { id: 's1', action: 'goto', url: '/login.html' },
        { id: 's2', action: 'observe_title' },
      ],
      { screenshots: 'every_step', record_video: true, record_trace: true },
    ),
  );
  const run = json as RunResponse;
  assert.equal(run.steps[0]?.screenshot, 's1.png');
  assert.equal(run.steps[1]?.screenshot, 's2.png');
  const kinds = Object.fromEntries(run.artifacts.map((a) => [a.name, a.kind]));
  assert.deepEqual(kinds, { 's1.png': 'screenshot', 's2.png': 'screenshot', 'trace.zip': 'trace', 'video.webm': 'video' });
  const video = run.artifacts.find((a) => a.kind === 'video');
  assert.ok((video?.size_bytes ?? 0) > 0);
  assert.equal(video?.content_type, 'video/webm');
});

test('run timeout aborts and skips the remaining steps', { skip }, async () => {
  const short = createRunnerServer({ port: 0, host: '127.0.0.1', token: null, runTimeoutMs: 800, maxConcurrency: 1, recorderHeadless: true, autoInstall: [] });
  const port = await listen(short.server);
  try {
    const res = await fetch(`http://127.0.0.1:${port}/v1/runs`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(
        runRequest([
          { id: 's1', action: 'goto', url: '/login.html' },
          { id: 's2', action: 'wait', ms: 5000 },
          { id: 's3', action: 'observe_title' },
        ]),
      ),
    });
    const run = (await res.json()) as RunResponse;
    assert.equal(run.status, 'aborted');
    assert.equal(run.steps[0]?.status, 'ok');
    assert.equal(run.steps[1]?.status, 'error');
    assert.equal(run.steps[1]?.error?.kind, 'timeout');
    assert.equal(run.steps[2]?.status, 'skipped');
    assert.ok(run.duration_ms < 4000, `duration ${run.duration_ms}`);
  } finally {
    await short.close();
  }
});

test('recorder script captures steps and redacts secrets', { skip }, async () => {
  const browser = await chromium.launch();
  try {
    const context = await browser.newContext();
    const events: unknown[] = [];
    await context.exposeBinding(BINDING_NAME, (_s, ev: unknown) => {
      events.push(ev);
    });
    await context.addInitScript({ content: recorderScript() });
    const page = await context.newPage();
    await page.goto(`${siteUrl}/login.html`);
    await page.getByLabel('Email').fill('demo@example.test');
    await page.getByLabel('Password').fill('hunter2');
    await page.getByLabel('Favourite colour').selectOption('blue');
    await page.getByLabel('Remember me').check();
    await page.getByPlaceholder('Search everything').fill('drill');
    await page.getByPlaceholder('Search everything').press('Enter');
    await page.waitForURL(/welcome/, { waitUntil: 'commit' });
    const plain = JSON.parse(JSON.stringify(events)) as Array<Record<string, unknown>>;
    const summary = plain.map((e) => [e['kind'], e['target'], e['value'] ?? e['key'] ?? null]);
    assert.deepEqual(summary, [
      ['fill', 'Email', 'demo@example.test'],
      ['fill', 'Password', '{{secret}}'],
      ['select', 'Favourite colour', 'blue'],
      ['check', 'Remember me', null],
      ['fill', 'Search everything', 'drill'],
      ['press', 'Search everything', 'Enter'],
    ]);
    assert.ok(!JSON.stringify(events).includes('hunter2'), 'raw password must never cross the binding');
  } finally {
    await browser.close();
  }
});

test('recording endpoints (headless mode)', { skip }, async () => {
  const created = await post('/v1/recordings', { url: `${siteUrl}/login.html` });
  assert.equal(created.status, 201, JSON.stringify(created.json));
  const id = (created.json as { recording_id: string }).recording_id;
  const get = await fetch(`${runnerUrl}/v1/recordings/${id}`, { headers: { authorization: `Bearer ${TOKEN}` } });
  const live = (await get.json()) as { status: string; events: Array<{ id: string; action: string; url?: string }> };
  assert.equal(live.status, 'recording');
  assert.deepEqual(live.events[0], { id: 'r1', action: 'goto', url: '/login.html' });
  const stopped = await post(`/v1/recordings/${id}/stop`, {});
  assert.equal((stopped.json as { status: string }).status, 'stopped');
  const missing = await fetch(`${runnerUrl}/v1/recordings/nope`, { headers: { authorization: `Bearer ${TOKEN}` } });
  assert.equal(missing.status, 404);
});
