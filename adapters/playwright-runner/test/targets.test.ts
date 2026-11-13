import { test } from 'node:test';
import assert from 'node:assert/strict';
import { targetStrategies } from '../src/targets.js';

test('css= prefix yields a single css strategy', () => {
  assert.deepEqual(targetStrategies('css=form > button.primary'), [{ kind: 'css', selector: 'form > button.primary' }]);
});

test('xpath= prefix yields a single xpath strategy', () => {
  assert.deepEqual(targetStrategies('xpath=//button[1]'), [{ kind: 'xpath', selector: '//button[1]' }]);
});

test('text= prefix yields exact text', () => {
  assert.deepEqual(targetStrategies('text=Sign in'), [{ kind: 'text', text: 'Sign in', exact: true }]);
});

test('selector-looking targets are treated as css', () => {
  for (const t of ['#email', '.btn', '[data-testid=price]', 'form >> button']) {
    assert.deepEqual(targetStrategies(t), [{ kind: 'css', selector: t }], t);
  }
});

test('plain text tries exact roles, partial roles, label, placeholder, test id, then text', () => {
  assert.deepEqual(targetStrategies('Sign in'), [
    { kind: 'role', role: 'button', name: 'Sign in', exact: true },
    { kind: 'role', role: 'link', name: 'Sign in', exact: true },
    { kind: 'role', role: 'button', name: 'Sign in', exact: false },
    { kind: 'role', role: 'link', name: 'Sign in', exact: false },
    { kind: 'label', text: 'Sign in' },
    { kind: 'placeholder', text: 'Sign in' },
    { kind: 'testid', id: 'Sign in' },
    { kind: 'text', text: 'Sign in', exact: false },
  ]);
});

test('a # inside plain text does not make it css', () => {
  assert.equal(targetStrategies('Order #42')[0]?.kind, 'role');
});

test('form intent (fill/select/check/uncheck) prefers label and placeholder', () => {
  assert.deepEqual(targetStrategies('Password', 'form'), [
    { kind: 'label', text: 'Password' },
    { kind: 'placeholder', text: 'Password' },
    { kind: 'testid', id: 'Password' },
    { kind: 'role', role: 'button', name: 'Password', exact: false },
    { kind: 'role', role: 'link', name: 'Password', exact: false },
    { kind: 'text', text: 'Password', exact: false },
  ]);
});

test('explicit selectors ignore intent', () => {
  assert.deepEqual(targetStrategies('#pw', 'form'), [{ kind: 'css', selector: '#pw' }]);
});
