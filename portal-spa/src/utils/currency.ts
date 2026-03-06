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

/** Format a USD value for display (e.g. "$16,621.50"). Returns null if input is null/undefined. */
export function formatUsdValue(
  value: string | null | undefined
): string | null {
  if (value == null) return null;
  const num = parseFloat(value);
  if (isNaN(num)) return null;
  const formatted = num.toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2
  });
  return `$${formatted}`;
}

/** Format an FX rate for display (e.g. "@ 66,486.00"). Returns null if rate is null or 1. */
export function formatFxRate(rate: string | null | undefined): string | null {
  if (rate == null) return null;
  const num = parseFloat(rate);
  if (isNaN(num) || num === 1) return null;
  const formatted = num.toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 8
  });
  return `@ ${formatted}`;
}

/** Whether a transaction is a debit (money leaving the account). */
export function isDebit(txType: string, description: string | null): boolean {
  if (txType === "withdrawal") return true;
  if (txType === "transfer" && description === "Transfer out") return true;
  return false;
}

/** Format a transaction amount with +/- prefix, respecting per-currency precision. */
export function formatAmount(
  amount: string,
  txType: string,
  currency: string,
  description?: string | null
): string {
  const num = parseFloat(amount);
  const prefix = isDebit(txType, description ?? null) ? "- " : "+ ";
  const formatted = Math.abs(num).toLocaleString("en-US", {
    minimumFractionDigits: minDecimals(currency),
    maximumFractionDigits: maxDecimals(currency)
  });
  return currency === "USD"
    ? `${prefix}$${formatted}`
    : `${prefix}${formatted}`;
}
