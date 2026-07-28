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

// Per-rail display chrome (Figma `cabinet/wallet` network cards): the chain's own name,
// the badge glyph, and the accent tier its badge is tinted with. The rails on offer come
// from `GET /api/wallet`, and an unlisted network falls back to its upper-cased id with a
// neutral badge — so a future rail renders sanely with no change here.
export interface RailMeta {
  label: string;
  chain: string;
  badge: string;
  tone: string;
}

const RAILS: Record<string, RailMeta> = {
  bep20: { label: "BEP20", chain: "BNB Smart Chain", badge: "B", tone: "bg-main-accent-t3/15 text-main-accent-t3" },
  trc20: { label: "TRC20", chain: "TRON", badge: "T", tone: "bg-main-accent-t4/15 text-main-accent-t4" },
  ton: { label: "TON", chain: "The Open Network", badge: "◆", tone: "bg-main-accent-t1/15 text-main-accent-t1" },
  polygon: { label: "Polygon", chain: "Polygon PoS", badge: "P", tone: "bg-main-accent-t1/15 text-main-accent-t1" },
};

export function railMeta(network: string | undefined): RailMeta {
  const id = network ?? "";
  return RAILS[id] ?? { label: id.toUpperCase(), chain: "Network", badge: (id[0] ?? "?").toUpperCase(), tone: "bg-muted text-muted-foreground" };
}

export function networkLabel(network: string | undefined): string {
  return railMeta(network).label;
}

// The EVM rails share the exact same `0x…` address format, so one rail's deposit address is a
// syntactically valid — but never credited — destination on the other. That collision is unique
// to the EVM rails (TON/TRON addresses look nothing alike), so the deposit view calls it out by
// name only for them.
const EVM_RAILS = new Set(["bep20", "polygon"]);

export function isEvmRail(network: string | undefined): boolean {
  return EVM_RAILS.has(network ?? "");
}
