// These tests encode the pricing SPEC from docs/requirements.md (US-8).
// One of them is marked `todo` because src/pricing.js contains a planted
// defect; it will start passing once the defect is fixed (see README).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { priceOrder, formatMoney } from '../src/pricing.js';

const line = (unit_price, quantity = 1) => ({ unit_price, quantity });

test('empty order costs nothing', () => {
  assert.deepEqual(priceOrder([]), { subtotal: 0, tax: 0, shipping: 0, total: 0, currency: 'USD' });
});

test('single item under the free-shipping threshold pays flat shipping', () => {
  // 12.50 + 8% tax (1.00) + 7.50 shipping
  assert.deepEqual(priceOrder([line(12.5)]), { subtotal: 12.5, tax: 1, shipping: 7.5, total: 21, currency: 'USD' });
});

test('single item at or above $100 ships free', () => {
  // 119.00 + 9.52 tax + free shipping
  assert.deepEqual(priceOrder([line(119)]), { subtotal: 119, tax: 9.52, shipping: 0, total: 128.52, currency: 'USD' });
});

test('several distinct lines add up and round to cents', () => {
  // 9.99 + 6.25 + 7.40 = 23.64; tax 1.8912 -> 1.89; shipping 7.50
  const totals = priceOrder([line(9.99), line(6.25), line(7.4)]);
  assert.equal(totals.subtotal, 23.64);
  assert.equal(totals.tax, 1.89);
  assert.equal(totals.shipping, 7.5);
  assert.equal(totals.total, 33.03);
});

test('quantities are honoured for subtotal, tax and the free-shipping threshold', { todo: 'planted defect' }, () => {
  // 2 x 12.50 = 25.00; tax 2.00; shipping 7.50; total 34.50
  assert.deepEqual(priceOrder([line(12.5, 2)]), { subtotal: 25, tax: 2, shipping: 7.5, total: 34.5, currency: 'USD' });
  // 2 x 54.00 = 108.00 crosses the threshold: tax 8.64; free shipping; total 116.64
  assert.deepEqual(priceOrder([line(54, 2)]), { subtotal: 108, tax: 8.64, shipping: 0, total: 116.64, currency: 'USD' });
});

test('formatMoney renders two decimals', () => {
  assert.equal(formatMoney(34.5), '$34.50');
  assert.equal(formatMoney(0), '$0.00');
});
