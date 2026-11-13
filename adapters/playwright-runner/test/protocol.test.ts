import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validateRunRequest, PROTOCOL } from '../src/protocol.js';

const minimal = () => ({
  protocol: PROTOCOL,
  run_id: '6f1c2c1e-1111-4c4c-9999-000000000001',
  base_url: 'http://localhost:4000',
  steps: [{ id: 's1', action: 'goto', url: '/' }],
});

test('minimal request is valid and gets defaults', () => {
  const r = validateRunRequest(minimal());
  assert.ok(r.ok);
  assert.equal(r.value.browser, 'chromium');
  assert.deepEqual(r.value.viewport, { width: 1280, height: 800 });
  assert.equal(r.value.step_timeout_ms, 15000);
  assert.equal(r.value.screenshots, 'on_failure');
  assert.equal(r.value.record_video, false);
  assert.equal(r.value.record_trace, false);
  assert.deepEqual(r.value.headers, {});
  assert.equal(r.value.storage_state, null);
  assert.deepEqual(r.value.allowed_hosts, []);
});

test('full request with every step type is valid', () => {
  const r = validateRunRequest({
    ...minimal(),
    browser: 'firefox',
    viewport: { width: 1024, height: 768 },
    step_timeout_ms: 5000,
    screenshots: 'every_step',
    record_video: true,
    record_trace: true,
    headers: { 'X-Foo': 'bar' },
    storage_state: { cookies: [], origins: [] },
    allowed_hosts: ['localhost'],
    steps: [
      { id: 'a', action: 'goto', url: '/login' },
      { id: 'b', action: 'click', target: 'Sign in' },
      { id: 'c', action: 'fill', target: 'Email', value: 'x' },
      { id: 'd', action: 'press', key: 'Enter' },
      { id: 'e', action: 'press', key: 'Tab', target: 'Email' },
      { id: 'f', action: 'select', target: 'Country', value: 'NO' },
      { id: 'g', action: 'check', target: 'Remember me' },
      { id: 'h', action: 'uncheck', target: 'Remember me' },
      { id: 'i', action: 'hover', target: 'Menu' },
      { id: 'j', action: 'wait', ms: 60000 },
      { id: 'k', action: 'screenshot', name: 'home' },
      { id: 'l', action: 'observe_text', text: 'Welcome', wait_ms: 0, visible: false },
      { id: 'm', action: 'observe_element', target: '#x', contains: 'y' },
      { id: 'n', action: 'observe_url', contains: '/dash' },
      { id: 'o', action: 'observe_title', equals: 'Home' },
    ],
  });
  assert.ok(r.ok, r.ok ? '' : r.message);
});

test('wrong protocol is unsupported_protocol', () => {
  const r = validateRunRequest({ ...minimal(), protocol: 'oryn.browser/v2' });
  assert.equal(r.ok, false);
  assert.equal(!r.ok && r.code, 'unsupported_protocol');
});

test('missing protocol and non-objects are invalid_request', () => {
  const { protocol: _omit, ...rest } = minimal();
  for (const body of [rest, null, [], 'x']) {
    const r = validateRunRequest(body);
    assert.equal(!r.ok && r.code, 'invalid_request');
  }
});

test('shape errors are invalid_request with a path in the message', () => {
  const cases: Array<[unknown, RegExp]> = [
    [{ ...minimal(), browser: 'safari' }, /browser/],
    [{ ...minimal(), base_url: 'ftp://x' }, /base_url/],
    [{ ...minimal(), steps: [] }, /steps/],
    [{ ...minimal(), steps: [{ id: 's1', action: 'teleport' }] }, /steps\.0/],
    [{ ...minimal(), steps: [{ id: 's1', action: 'click' }] }, /target/],
    [{ ...minimal(), steps: [{ id: 's1', action: 'goto', url: '/', extra: 1 }] }, /steps\.0/],
    [{ ...minimal(), screenshots: 'always' }, /screenshots/],
    [{ ...minimal(), unknown_field: true }, /unknown_field|Unrecognized/],
    [{ ...minimal(), step_timeout_ms: 0 }, /step_timeout_ms/],
    [
      { ...minimal(), steps: [{ id: 's1', action: 'goto', url: '/' }, { id: 's1', action: 'observe_url' }] },
      /duplicate step id/,
    ],
  ];
  for (const [body, pattern] of cases) {
    const r = validateRunRequest(body);
    assert.equal(r.ok, false, JSON.stringify(body));
    if (!r.ok) {
      assert.equal(r.code, 'invalid_request');
      assert.match(r.message, pattern);
    }
  }
});
