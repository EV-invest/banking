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

const NETWORK_LABELS: Record<string, string> = { bep20: "BEP20", trc20: "TRC20", ton: "TON" };

export function networkLabel(network: string | undefined): string {
  return NETWORK_LABELS[network ?? ""] ?? (network ?? "").toUpperCase();
}

export const NETWORKS = ["bep20", "trc20", "ton"] as const;
