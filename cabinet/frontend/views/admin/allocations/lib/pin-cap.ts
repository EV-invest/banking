// "Pin cap to issued": close a product's supply at exactly what is out. Pure, so the three
// rules that make the button safe to offer — nothing to pin while an emission is still in
// the relay, nothing to pin while nothing has settled, nothing to do when the cap already
// sits there — are tested here rather than discovered against a live product.

// Relative and with the extension, like `views/admin/payments/lib/terms.ts`: the node
// test runner resolves no `@/` alias.
import { toBaseUnits } from "../../../../shared/lib/money.ts";

export type PinCapVerdict =
  /** The cap would move from `from` to `to`; `to` is the wire figure to send as-is. */
  | { kind: "pinnable"; from: string; to: string }
  /** `queued` units sit in the relay, accepted but not yet posted. Pinning at the settled
   *  figure would put the cap below the real supply the moment they land; pinning at
   *  settled + queued is no better, because the relay may still park or reject them and
   *  the cap would then sit above the real supply. Neither figure is the truth yet, so
   *  the only safe move is to wait. Takes precedence over `nothingIssued`: "the queue
   *  is still open" is the reason the operator can act on. */
  | { kind: "queuedPending"; queued: string }
  /** Nothing has landed on the ledger and nothing is in the relay either. */
  | { kind: "nothingIssued" }
  /** The cap already equals the settled supply. */
  | { kind: "alreadyPinned" };

export function pinCapVerdict(unitCap: string | undefined, unitsOutstanding: string | undefined, queuedUnits: string | undefined): PinCapVerdict {
  // `toBaseUnits` reads undefined and "" as zero, which is exactly the fold an object
  // cached before the BFF sent `queued_units` needs.
  if (toBaseUnits(queuedUnits) > 0n) return { kind: "queuedPending", queued: (queuedUnits ?? "0").trim() };
  const outstanding = toBaseUnits(unitsOutstanding);
  if (outstanding <= 0n) return { kind: "nothingIssued" };
  // Exact comparison on the wire decimals: "16250" and "16250.000" are one figure.
  if (toBaseUnits(unitCap) === outstanding) return { kind: "alreadyPinned" };
  return { kind: "pinnable", from: unitCap ?? "0", to: (unitsOutstanding ?? "0").trim() };
}
