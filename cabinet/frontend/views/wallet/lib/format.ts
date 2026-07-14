// Display helpers for wallet amounts and addresses. Amounts cross the wire as exact
// decimal USDT strings; these format for display only (the authoritative value is the
// string). Never use the parsed number for anything but rendering.

export function formatUsdt(value: string | undefined): string {
  const n = Number(value ?? "0");
  if (!Number.isFinite(n)) return value ?? "0";
  return n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 6 });
}

export function shortAddress(address: string | undefined): string {
  if (!address) return "—";
  return address.length > 18 ? `${address.slice(0, 8)}…${address.slice(-6)}` : address;
}

// Exact 18-decimal USDT math on decimal strings (display-only previews). Avoids the
// float error Number() introduces on 18-dp values — money stays exact end to end.
const USDT_DECIMALS = 18;

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

// Display names only — the rails on offer come from `GET /api/wallet`, and an unlisted
// network falls back to its upper-cased id, so a future rail needs no change here.
const NETWORK_LABELS: Record<string, string> = { bep20: "BEP20", trc20: "TRC20", ton: "TON", polygon: "Polygon" };

export function networkLabel(network: string | undefined): string {
  return NETWORK_LABELS[network ?? ""] ?? (network ?? "").toUpperCase();
}
