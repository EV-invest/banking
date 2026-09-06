// The cabinet's one money module. Amounts cross the wire as exact decimal strings (the
// authoritative value is the string); everything here is display-only — never feed a
// parsed number back into a money operation. The exact-math helpers at the bottom are the
// only place a money value is ever computed, and they work on bigint base units.
//
// Precision is a policy per unit of measure, not a per-screen choice, because the same
// amount used to render differently depending on which screen you were on:
//
//   summary money (USD)   exactly 2 dp  — a fund that rounds a P&L to the dollar reads
//                                         as an estimate rather than a statement
//   ledger money (USDT)   2–6 dp        — a wallet row has to show what actually moved
//   fund units (shares)   2–8 dp        — fractional shares read clearly
//   NAV per share         exactly 4 dp  — a NAV is a price; the day's move lives in the
//                                         fourth decimal

/** A wire decimal parsed for on-screen arithmetic (sums, proportions). Display only. */
export function num(value: string | undefined): number {
  const n = Number(value ?? "0");
  return Number.isFinite(n) ? n : 0;
}

// Summary money is shown to the cent, always — a trailing "$1,234.5" breaks the column
// and a rounded "$85" loses the P&L.
const CENTS = { minimumFractionDigits: 2, maximumFractionDigits: 2 } as const;

/** Summary money: "$48,250.00", "$1,234.56". Dashboards, profile and the admin console. */
export function formatUsd(value: string | number | undefined): string {
  const n = typeof value === "number" ? value : num(value);
  return n.toLocaleString("en-US", { style: "currency", currency: "USD", ...CENTS });
}

/** The same figure without the currency symbol, for columns that name the unit elsewhere. */
export function formatAmount(value: string | undefined): string {
  const n = Number(value ?? "0");
  if (!Number.isFinite(n)) return value ?? "0";
  return n.toLocaleString("en-US", CENTS);
}

// Signed summary money: "+$84.83" / "−$540.00". The Unicode minus (U+2212) is the plus
// sign's mirror — a hyphen is narrower and the sign column stops lining up.
export function formatSignedUsd(value: string | number | undefined): string {
  const n = typeof value === "number" ? value : num(value);
  return `${n < 0 ? "−" : "+"}${formatUsd(Math.abs(n))}`;
}

/** NAV per share: "$1.0423" (Figma `cabinet/invest`). */
export function formatNav(value: string | number | undefined): string {
  const n = typeof value === "number" ? value : num(value);
  return n.toLocaleString("en-US", { style: "currency", currency: "USD", minimumFractionDigits: 4, maximumFractionDigits: 4 });
}

/**
 * A wire decimal shown EXACTLY as it arrived, grouped for reading: "1,234.50",
 * "1,000.0000005", "0.000000000000000001".
 *
 * The other formatters here parse to a `number` first, which is right for a balance — a
 * float is plenty for a figure the reader is only comparing against other figures, and
 * capping the fraction is what keeps a column aligned. It is wrong for an amount the reader
 * is *authorizing*: `formatUsdt` caps at 6 dp, so an 18-decimal value is silently rounded on
 * screen, and `Number()` itself loses digits past ~15 significant figures.
 *
 * That matters on exactly one surface and it matters absolutely. `payload_hash` is computed
 * over the exact decimal string and re-verified before the money moves, so what an owner
 * approves must be what the hash covers, character for character (docs/CONSILIUM.md,
 * policy 12). An approval screen that rounds is an approval of something else.
 *
 * No float anywhere: the integer part is grouped through `BigInt`, which has no precision
 * ceiling, and the fraction is passed through untouched apart from being padded to the two
 * decimals every money figure in this cabinet shows. Grouping is `en-US` for the same
 * reason the rest of this module pins it.
 */
export function formatExactUsdt(value: string | undefined): string {
  const raw = (value ?? "").trim();
  // Anything that is not a plain decimal is returned as-is rather than coerced: a caller
  // showing an unrecognised value verbatim is honest, one showing "0.00" for it is not.
  if (!/^-?\d*\.?\d*$/.test(raw) || raw === "" || raw === "." || raw === "-" || raw === "-.") return raw || "—";
  const negative = raw.startsWith("-");
  const [intRaw = "", fracRaw = ""] = raw.replace(/^-/, "").split(".");
  const grouped = BigInt(intRaw || "0").toLocaleString("en-US");
  const frac = fracRaw.length < 2 ? fracRaw.padEnd(2, "0") : fracRaw;
  return `${negative ? "\u2212" : ""}${grouped}.${frac}`;
}

/** Ledger USDT: "1,234.50", "0.000001". No currency symbol — the unit is spelled out. */
export function formatUsdt(value: string | undefined): string {
  const n = Number(value ?? "0");
  if (!Number.isFinite(n)) return value ?? "0";
  return n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 6 });
}

// Signed USDT (P&L): "+1,234.50" / "-5.00". Handles a leading "-" in the wire string and
// keeps the sign explicit so gains/losses read at a glance.
export function formatSignedUsdt(value: string | undefined): string {
  const s = (value ?? "0").trim();
  const negative = s.startsWith("-");
  const formatted = formatUsdt(negative ? s.slice(1) : s);
  return `${negative ? "-" : "+"}${formatted}`;
}

// Fund units (shares) — same dimension as USDT for display, but no currency suffix and a
// touch more precision so fractional shares read clearly.
export function formatUnits(value: string | undefined): string {
  const n = Number(value ?? "0");
  if (!Number.isFinite(n)) return value ?? "0";
  return n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 8 });
}

/** Signed percentage: "+4.2%" / "−1.8%", same Unicode minus as the signed money. */
export function formatPct(value: number): string {
  return `${value < 0 ? "−" : "+"}${Math.abs(value).toFixed(1)}%`;
}

// Whether a signed decimal P&L string is negative (a loss) — exact, no float.
export function isNegative(value: string | undefined): boolean {
  return (value ?? "").trim().startsWith("-");
}

// Whether a decimal P&L string is exactly zero (treated as a gain for colour).
export function isZero(value: string | undefined): boolean {
  return toBaseUnits((value ?? "").trim().replace(/^-/, "")) === 0n;
}

// Exact 18-decimal USDT math on decimal strings (display-only previews). Avoids the
// float error Number() introduces on 18-dp values — money stays exact end to end.
export const USDT_DECIMALS = 18;

export function toBaseUnits(value: string | undefined): bigint {
  const s = (value ?? "").trim();
  if (!s || !/^\d*\.?\d*$/.test(s)) return 0n;
  const [int = "0", frac = ""] = s.split(".");
  const fracPadded = (frac + "0".repeat(USDT_DECIMALS)).slice(0, USDT_DECIMALS);
  try {
    return BigInt(int || "0") * 10n ** BigInt(USDT_DECIMALS) + BigInt(fracPadded || "0");
  } catch {
    return 0n;
  }
}

export function fromBaseUnits(units: bigint): string {
  const scale = 10n ** BigInt(USDT_DECIMALS);
  const abs = units < 0n ? -units : units;
  const int = abs / scale;
  const frac = (abs % scale).toString().padStart(USDT_DECIMALS, "0").replace(/0+$/, "");
  return `${units < 0n ? "-" : ""}${int}${frac ? `.${frac}` : ""}`;
}

// Saturating `a - b` over decimal USDT strings, returned as a decimal string (never < 0).
export function subUsdt(a: string | undefined, b: string | undefined): string {
  const r = toBaseUnits(a) - toBaseUnits(b);
  return fromBaseUnits(r < 0n ? 0n : r);
}

// Unit counts get large — a hundred-million-unit cap is unreadable written out, and the
// figure it is compared against has to be readable at the same glance. Compacts to 3
// significant figures ("21.0M", "940K", "1.5B") and only above a thousand, so a fund
// sized to 500 units still reads as "500".
export function compactUnits(value: string | undefined): string {
  const n = num(value);
  if (!Number.isFinite(n) || n < 1000) return formatUnits(value);
  for (const [scale, suffix] of [[1e12, "T"], [1e9, "B"], [1e6, "M"], [1e3, "K"]] as const) {
    if (n >= scale) {
      const scaled = n / scale;
      return `${scaled >= 100 ? Math.round(scaled) : Number(scaled.toFixed(scaled >= 10 ? 1 : 2))}${suffix}`;
    }
  }
  return formatUnits(value);
}

// What fraction of `cap` is taken by `issued`, 0–1, for a progress bar. Exact bigint
// division — a 1e26 cap overflows a float's integer precision, so
// `Number(issued) / Number(cap)` would quietly lie at the sizes this feature is for.
//
// The scale is 1e12 rather than basis points because these caps are large: one unit of a
// hundred-million-unit fund is 1e-8, which basis-point division floors to a flat zero.
// That is fine for a bar (it is invisible either way) and wrong for anything that asks
// this function a question about a small holding.
const CAP_FRACTION_SCALE = 1_000_000_000_000n;

export function fractionOfCap(issued: string | undefined, cap: string | undefined): number {
  const a = toBaseUnits(issued);
  const b = toBaseUnits(cap);
  if (b <= 0n) return 0;
  if (a >= b) return 1;
  return Number((a * CAP_FRACTION_SCALE) / b) / Number(CAP_FRACTION_SCALE);
}

// How much of an address survives truncation. The wallet (the default) keeps enough of a
// deposit address to check it against a wallet app; the dashboard's activity lines have
// far less room and cut harder. `min` rides along because each screen also picked its own
// length below which an address is left whole.
export interface AddressCut {
  head?: number;
  tail?: number;
  min?: number;
}

export function shortAddress(address: string | undefined, cut: AddressCut = {}): string {
  const { head = 8, tail = 6, min = 18 } = cut;
  if (!address) return "—";
  return address.length > min ? `${address.slice(0, head)}…${address.slice(-tail)}` : address;
}
