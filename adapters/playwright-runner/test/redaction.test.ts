import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isSensitiveField } from '../src/redaction.js';
import { networkDecision } from '../src/policy.js';
import { Semaphore } from '../src/semaphore.js';

test('password inputs are always sensitive', () => {
  assert.equal(isSensitiveField({ type: 'password', name: 'x' }), true);
  assert.equal(isSensitiveField({ type: 'PASSWORD' }), true);
});

test('name, id, autocomplete or label matching the pattern is sensitive', () => {
  const cases = [
    { name: 'user_pass' },
    { id: 'apiSecret' },
    { autocomplete: 'cc-number', label: 'Card number' },
    { autocomplete: 'one-time-code', name: 'otp' },
    { label: 'CVV' },
    { name: 'ssn' },
    { id: 'reset-token' },
    { autocomplete: 'current-password' },
  ];
  for (const f of cases) assert.equal(isSensitiveField({ type: 'text', ...f }), true, JSON.stringify(f));
});

test('ordinary fields are not sensitive', () => {
  assert.equal(isSensitiveField({ type: 'email', name: 'email', id: 'email', autocomplete: 'username', label: 'Email' }), false);
  assert.equal(isSensitiveField({ type: 'text', name: 'q', label: 'Search products' }), false);
  assert.equal(isSensitiveField({}), false);
});

test('network policy: allow-list, always-allowed schemes, metadata IP', () => {
  const allow = ['localhost', '127.0.0.1'];
  assert.equal(networkDecision('http://localhost:4000/x', allow), 'allow');
  assert.equal(networkDecision('http://LOCALHOST/x', allow), 'allow');
  assert.equal(networkDecision('https://cdn.example.com/a.js', allow), 'block');
  assert.equal(networkDecision('data:text/plain,hi', allow), 'allow');
  assert.equal(networkDecision('blob:http://localhost/abc', allow), 'allow');
  assert.equal(networkDecision('https://cdn.example.com/a.js', []), 'allow');
  assert.equal(networkDecision('http://169.254.169.254/latest/meta-data', []), 'block');
  assert.equal(networkDecision('http://169.254.169.254/', ['169.254.169.254']), 'block');
  assert.equal(networkDecision('http://[::1]:3000/', ['::1']), 'allow');
});

test('semaphore bounds concurrency and runs queued tasks in order', async () => {
  const sem = new Semaphore(2);
  let active = 0;
  let peak = 0;
  const order: number[] = [];
  await Promise.all(
    [0, 1, 2, 3, 4].map((i) =>
      sem.run(async () => {
        active++;
        peak = Math.max(peak, active);
        await new Promise((r) => setTimeout(r, 10));
        order.push(i);
        active--;
      }),
    ),
  );
  assert.equal(peak, 2);
  assert.deepEqual(order, [0, 1, 2, 3, 4]);
});

test('wildcard entries match the domain and its subdomains, like the Rust policy', () => {
  const allow = ['*.example.com'];
  assert.equal(networkDecision('https://example.com/', allow), 'allow');
  assert.equal(networkDecision('https://app.example.com/', allow), 'allow');
  assert.equal(networkDecision('https://a.b.example.com/', allow), 'allow');
  assert.equal(networkDecision('https://badexample.com/', allow), 'block');
  assert.equal(networkDecision('https://example.com.evil.test/', allow), 'block');
});

test('every cloud metadata host is blocked even with no allow list', () => {
  for (const h of ['169.254.169.254', 'metadata.google.internal', '100.100.100.200', '[fd00:ec2::254]']) {
    assert.equal(networkDecision(`http://${h}/latest`, []), 'block', h);
  }
});
