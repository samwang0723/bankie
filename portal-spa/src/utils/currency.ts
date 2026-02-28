/** Per-currency decimal precision matching backend (crates/bankie-core/src/common/money.rs) */
const CURRENCY_PRECISION: Record<string, number> = {
  USD: 2,
  TWD: 0,
  BTC: 8,
  ETH: 18,
  USDT: 6
};

/** Max display decimals — ETH's 18 is too many for UI */
const MAX_DISPLAY_DECIMALS = 8;

function maxDecimals(currency: string): number {
  const p = CURRENCY_PRECISION[currency] ?? 2;
  return Math.min(p, MAX_DISPLAY_DECIMALS);
}

/** Fiat currencies use fixed decimals (e.g. USD always 2, TWD always 0).
 *  Crypto currencies show up to maxDecimals but trim trailing zeros (min 2). */
function minDecimals(currency: string): number {
  const p = CURRENCY_PRECISION[currency] ?? 2;
  // Fiat: fixed precision (USD=2, TWD=0)
  if (currency === "USD" || currency === "TWD") return p;
  // Crypto: at least 2 for readability, but never more than max
  return Math.min(2, maxDecimals(currency));
}

/** Format a balance value for display, respecting per-currency precision. */
export function formatBalance(
  value: string | undefined,
  currency: string
): string {
  if (value == null) return "--";
  const num = parseFloat(value);
  if (isNaN(num)) return "--";
  const formatted = num.toLocaleString("en-US", {
    minimumFractionDigits: minDecimals(currency),
    maximumFractionDigits: maxDecimals(currency)
  });
  return currency === "USD" ? `$${formatted}` : formatted;
}

/** Format a transaction amount with +/- prefix, respecting per-currency precision. */
export function formatAmount(
  amount: string,
  txType: string,
  currency: string
): string {
  const num = parseFloat(amount);
  const prefix = txType === "withdrawal" ? "- " : "+ ";
  const formatted = Math.abs(num).toLocaleString("en-US", {
    minimumFractionDigits: minDecimals(currency),
    maximumFractionDigits: maxDecimals(currency)
  });
  return currency === "USD"
    ? `${prefix}$${formatted}`
    : `${prefix}${formatted}`;
}
