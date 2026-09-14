// Which of three things the plane's answer to "schedule this change" actually says.
//
// Deliberately import-free, like `shared/lib/unix-stamp.ts`: `node --test` does not resolve
// the `@/` alias, and the distinction below is exactly the one that must stay testable —
// the receipt used to know only "awaiting the owners" and "scheduled", and told the
// operator of a product nobody holds that every holder was being emailed.

export type ReceiptKind = "awaiting" | "scheduled" | "immediate";

/**
 * `awaiting`: the owners have to agree first. `scheduled`: a moment has to arrive, and the
 * holders are being told. `immediate`: nobody holds units, so the plane owed no notice and
 * set the moment to the request itself — the sweeper promotes it within a minute, and
 * there is no one to email. The third is told apart by the moment, not by a state: the wire
 * says `scheduled` for both, with `effective_from == scheduled_at` (or already past) only
 * when no notice was due.
 */
export function receiptKind(change: { state: string; effective_from: string; scheduled_at: string }, nowSeconds: number): ReceiptKind {
  if (change.state === "awaiting_consilium") return "awaiting";
  const effective = Number(change.effective_from);
  if (!Number.isFinite(effective) || effective <= 0) return "scheduled";
  if (effective <= nowSeconds || change.effective_from === change.scheduled_at) return "immediate";
  return "scheduled";
}
