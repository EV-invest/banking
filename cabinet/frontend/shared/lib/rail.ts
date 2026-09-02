// Per-rail display chrome: the chain's own name, the badge glyph, and the accent tier its
// badge is tinted with.
//
// This moved out of `views/wallet/lib/format.ts` when the governance surfaces arrived. It
// had been a wallet detail for as long as only the wallet named a chain; now the payout
// approval page and the owners' room both render a network beside an address, and a rail's
// display name is not the wallet slice's property to lend them — a `views/*` importing
// another `views/*` is a same-layer dependency, and the layer below is where a fact shared
// by three slices belongs. `views/wallet/lib/format.ts` re-exports these, so every existing
// call site is unchanged.
//
// `label` and `badge` are the rail's own marks — `BEP20`, `TON`, `◆` — and stay literal in
// every locale. The chain's *name* is prose, so it travels as a catalogue key resolved at
// the render site: this module is plain TypeScript and has no translator of its own.

export interface RailMeta {
  label: string;
  chainKey: string;
  badge: string;
  tone: string;
}

const RAILS: Record<string, RailMeta> = {
  bep20: { label: "BEP20", chainKey: "wallet.chain.bep20", badge: "B", tone: "bg-main-accent-t3/15 text-main-accent-t3" },
  trc20: { label: "TRC20", chainKey: "wallet.chain.trc20", badge: "T", tone: "bg-main-accent-t4/15 text-main-accent-t4" },
  ton: { label: "TON", chainKey: "wallet.chain.ton", badge: "◆", tone: "bg-main-accent-t1/15 text-main-accent-t1" },
  polygon: { label: "Polygon", chainKey: "wallet.chain.polygon", badge: "P", tone: "bg-main-accent-t1/15 text-main-accent-t1" },
};

export function railMeta(network: string | undefined): RailMeta {
  const id = network ?? "";
  return RAILS[id] ?? { label: id.toUpperCase(), chainKey: "wallet.chain.unknown", badge: (id[0] ?? "?").toUpperCase(), tone: "bg-muted text-muted-foreground" };
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
