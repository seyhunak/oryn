import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import type { Browser } from 'playwright';
import { BrowserPool, isMissingExecutable } from '../src/browsers.js';
import { parseAutoInstall } from '../src/config.js';
import type { BrowserName } from '../src/protocol.js';

const MISSING = "browserType.launch: Executable doesn't exist at /cache/chromium-9999/chrome";

function fakeBrowser(): Browser {
  return Object.assign(new EventEmitter(), { version: () => '1.0', close: async () => undefined }) as unknown as Browser;
}

/** Launch fails with "missing executable" until `installed` contains the engine. */
function fakes() {
  const installed = new Set<BrowserName>();
  const installs: BrowserName[] = [];
  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  return {
    installed,
    installs,
    release,
    launch: async (name: BrowserName) => {
      if (!installed.has(name)) throw new Error(`${MISSING}\nPlease run the following command to download new browsers`);
      return fakeBrowser();
    },
    install: async (name: BrowserName) => {
      installs.push(name);
      await gate;
      installed.add(name);
    },
  };
}

test('isMissingExecutable recognises the Playwright download hint', () => {
  assert.ok(isMissingExecutable(new Error(MISSING)));
  assert.ok(!isMissingExecutable(new Error('Target page, context or browser has been closed')));
});

test('a missing engine is installed once, reported unavailable meanwhile, then available', async () => {
  const f = fakes();
  const pool = new BrowserPool({ autoInstall: ['chromium'], launch: f.launch, install: f.install });

  const during = await pool.availability();
  const chromium = during.find((b) => b.name === 'chromium');
  assert.equal(chromium?.available, false);
  assert.match(chromium?.error ?? '', /installing/);
  await pool.availability();
  assert.deepEqual(f.installs, ['chromium'], 'install starts once and only for auto-install engines');

  const waiting = pool.get('chromium');
  f.release();
  assert.equal((await waiting).version(), '1.0', 'get() waits for the install and then launches');

  const after = await pool.availability();
  assert.equal(after.find((b) => b.name === 'chromium')?.available, true);
  assert.equal(after.find((b) => b.name === 'firefox')?.available, false);
  await pool.close();
});

test('engines outside autoInstall are never installed', async () => {
  const f = fakes();
  const pool = new BrowserPool({ autoInstall: [], launch: f.launch, install: f.install });
  await assert.rejects(pool.get('chromium'), /Executable doesn't exist/);
  assert.deepEqual(f.installs, []);
});

test('a failed install is not retried and get() reports the launch error', async () => {
  let calls = 0;
  const pool = new BrowserPool({
    autoInstall: ['chromium'],
    launch: async () => {
      throw new Error(MISSING);
    },
    install: async () => {
      calls++;
      throw new Error('network down');
    },
  });
  await assert.rejects(pool.get('chromium'), /Executable doesn't exist/);
  await assert.rejects(pool.get('chromium'), /Executable doesn't exist/);
  assert.equal(calls, 1);
});

test('ORYN_RUNNER_AUTO_INSTALL parsing', () => {
  assert.deepEqual(parseAutoInstall(undefined), ['chromium']);
  assert.deepEqual(parseAutoInstall('none'), []);
  assert.deepEqual(parseAutoInstall(''), []);
  assert.deepEqual(parseAutoInstall('chromium, Firefox,chromium'), ['chromium', 'firefox']);
  assert.throws(() => parseAutoInstall('edge'), /unknown browser/);
});
