// Display helpers for fund-shares amounts. NAV/value/cash are decimal USDT strings and
// reuse the wallet's exact-math helpers; units (shares) are a separate dimension. As in
// the wallet slice, the authoritative value is the string — these format for display only.

import { formatUsdt, toBaseUnits } from "@/views/wallet/lib/format";

// Re-export the wallet money helpers so the invest slice has one import surface.
export { formatUsdt, fromBaseUnits, subUsdt, toBaseUnits } from "@/views/wallet/lib/format";

// Fund units (shares) — same dimension as USDT for display, but no currency suffix and a
// touch more precision so fractional shares read clearly.
export function formatUnits(value: string | undefined): string {
  const n = Number(value ?? "0");
  if (!Number.isFinite(n)) return value ?? "0";
  return n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 8 });
}

// Signed USDT (P&L): "+1,234.50" / "-5.00". Handles a leading "-" in the wire string and
// keeps the sign explicit so gains/losses read at a glance.
export function formatSignedUsdt(value: string | undefined): string {
  const s = (value ?? "0").trim();
  const negative = s.startsWith("-");
  const formatted = formatUsdt(negative ? s.slice(1) : s);
  return `${negative ? "-" : "+"}${formatted}`;
}

// Whether a signed decimal P&L string is negative (a loss) — exact, no float.
export function isNegative(value: string | undefined): boolean {
  return (value ?? "").trim().startsWith("-");
}

// Whether a decimal P&L string is exactly zero (treated as a gain for colour).
export function isZero(value: string | undefined): boolean {
  return toBaseUnits((value ?? "").trim().replace(/^-/, "")) === 0n;
}
