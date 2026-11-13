// Northwind Supply pricing rules.
//
// Spec (docs/requirements.md, story US-8):
//   subtotal = sum of unit price x quantity over all lines
//   tax      = 8% of the subtotal
//   shipping = free when the subtotal is $100.00 or more, otherwise $7.50
//   total    = subtotal + tax + shipping, rounded to cents
//
// All arithmetic is done in integer cents to avoid floating point drift.

export const CURRENCY = 'USD';
export const TAX_RATE = 0.08;
export const FREE_SHIPPING_THRESHOLD = 100;
export const FLAT_SHIPPING = 7.5;

const toCents = (amount) => Math.round(amount * 100);
const fromCents = (cents) => Math.round(cents) / 100;

/**
 * Value of a single cart line in cents.
 * @param {{ unit_price: number, quantity: number }} line
 */
export function lineTotalCents(line) {
  return toCents(line.unit_price) * line.quantity;
}

/**
 * Promotions and tax are assessed on the qualifying merchandise value.
 * @param {Array<{ unit_price: number, quantity: number }>} lines
 */
function qualifyingValueCents(lines) {
  return lines.reduce((sum, { unit_price }) => sum + toCents(unit_price), 0);
}

/**
 * Price an order.
 * @param {Array<{ unit_price: number, quantity: number }>} lines
 * @returns {{ subtotal: number, tax: number, shipping: number, total: number, currency: string }}
 */
export function priceOrder(lines) {
  const subtotalCents = lines.reduce((sum, line) => sum + lineTotalCents(line), 0);
  const qualifyingCents = qualifyingValueCents(lines);

  const taxCents = Math.round(qualifyingCents * TAX_RATE);
  const shippingCents =
    lines.length === 0 || qualifyingCents >= toCents(FREE_SHIPPING_THRESHOLD)
      ? 0
      : toCents(FLAT_SHIPPING);
  const totalCents = subtotalCents + taxCents + shippingCents;

  return {
    subtotal: fromCents(subtotalCents),
    tax: fromCents(taxCents),
    shipping: fromCents(shippingCents),
    total: fromCents(totalCents),
    currency: CURRENCY,
  };
}

/** Format a dollar amount for display, e.g. 34.5 -> "$34.50". */
export function formatMoney(amount) {
  return `$${amount.toFixed(2)}`;
}
