/** Per-currency decimal precision matching backend (crates/bankie-core/src/common/money.rs) */
const CURRENCY_PRECISION: Record<string, number> = {
  USD: 2,
  TWD: 0,
  BTC: 8,
  ETH: 18,
  USDT: 6,
};

/** Max display decimals — ETH's 18 is too many for UI */
const MAX_DISPLAY_DECIMALS = 8;

function precision(currency: string): number {
  const p = CURRENCY_PRECISION[currency] ?? 2;
  return Math.min(p, MAX_DISPLAY_DECIMALS);
}

function isFiat(currency: string): boolean {
  return currency === "USD" || currency === "TWD";
}

/** Format a balance value for display, respecting per-currency precision. */
export function formatBalance(
  value: string | undefined,
  currency: string
): string {
  if (value == null) return "--";
  const num = parseFloat(value);
  if (isNaN(num)) return "--";
  const decimals = precision(currency);
  const formatted = num.toLocaleString("en-US", {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
  return isFiat(currency) && currency !== "TWD"
    ? `$${formatted}`
    : formatted;
}

/** Format a transaction amount with +/- prefix, respecting per-currency precision. */
export function formatAmount(
  amount: string,
  txType: string,
  currency: string
): string {
  const num = parseFloat(amount);
  const prefix = txType === "withdrawal" ? "- " : "+ ";
  const decimals = precision(currency);
  const formatted = Math.abs(num).toLocaleString("en-US", {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
  return isFiat(currency) && currency !== "TWD"
    ? `${prefix}$${formatted}`
    : `${prefix}${formatted}`;
}
