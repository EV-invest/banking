// Display helpers for the portfolio dashboard. Amounts arrive as exact decimal USDT
// strings (the authoritative value lives server-side); these parse only for on-screen
// figures and proportions — never feed the parsed number back into a money operation.

export function num(value: string | undefined): number {
  const n = Number(value ?? "0");
  return Number.isFinite(n) ? n : 0;
}

// Headline/stat money: "$48,250" (whole dollars — the figures are summaries, not ledger
// entries). Uses the Unicode minus elsewhere for sign symmetry.
export function formatMoney(value: string | number | undefined): string {
  const n = typeof value === "number" ? value : num(value);
  return n.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });
}

export function formatSignedMoney(value: number): string {
  return `${value < 0 ? "−" : "+"}${formatMoney(Math.abs(value))}`;
}

export function formatPct(value: number): string {
  return `${value < 0 ? "−" : "+"}${Math.abs(value).toFixed(1)}%`;
}

export function shortAddress(address: string | undefined): string {
  if (!address) return "—";
  return address.length > 16 ? `${address.slice(0, 6)}…${address.slice(-4)}` : address;
}
