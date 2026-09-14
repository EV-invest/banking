// "Pin cap to issued": close a product's supply at exactly what is out. Pure, so the two
// rules that make the button safe to offer — nothing to pin while nothing has settled,
// nothing to do when the cap already sits there — are tested here rather than discovered
// against a live product.

// Relative and with the extension, like `views/admin/payments/lib/terms.ts`: the node
// test runner resolves no `@/` alias.
import { toBaseUnits } from "../../../../shared/lib/money.ts";

export type PinCapVerdict =
  /** The cap would move from `from` to `to`; `to` is the wire figure to send as-is. */
  | { kind: "pinnable"; from: string; to: string }
  /** Nothing has landed on the ledger. A `queued` mint is not out yet, so pinning now
   *  would close the supply at zero and the relay's own post would then exceed it. */
  | { kind: "nothingIssued" }
  /** The cap already equals the settled supply. */
  | { kind: "alreadyPinned" };

export function pinCapVerdict(unitCap: string | undefined, unitsOutstanding: string | undefined): PinCapVerdict {
  const outstanding = toBaseUnits(unitsOutstanding);
  if (outstanding <= 0n) return { kind: "nothingIssued" };
  // Exact comparison on the wire decimals: "16250" and "16250.000" are one figure.
  if (toBaseUnits(unitCap) === outstanding) return { kind: "alreadyPinned" };
  return { kind: "pinnable", from: unitCap ?? "0", to: (unitsOutstanding ?? "0").trim() };
}
