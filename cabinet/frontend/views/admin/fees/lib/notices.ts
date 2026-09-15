// What a change's notice figures say — four things, and the order they are read in.
//
// Import-free like `./receipt.ts`, so `node --test` can exercise the two decisions that
// matter. A waiver outranks the count: after an acknowledgement the hub keeps reporting
// the undelivered notices (they are still undelivered), and a card that read the count
// first would offer to take responsibility for holders somebody already took it for. And
// a notice still in the mailer's queue is not one the holder "could not be told" — right
// after scheduling every notice is undelivered for the half-minute before the first
// attempt, and the hub refuses an acknowledgement until it has given up on at least one.

export type NoticeSummary =
  /** Nothing to say: every notice delivered, or a state in which none are counted. */
  | { kind: "none" }
  /** Notices the mailer is still trying to deliver, none given up on: nothing to do yet. */
  | { kind: "queued"; queued: number }
  /** Holders the mailer has given up on — the ones an acknowledgement would cover — and
   *  how many notices beside theirs are still being tried. */
  | { kind: "givenUp"; givenUp: number; queued: number }
  /** Somebody took responsibility: who, when (unix seconds as a string), and over how many. */
  | { kind: "waived"; by: string; at: string; holders: number };

export interface NoticeSource {
  state: string;
  notices_waived_by: string | null;
  notices_waived_at: string;
  notices_waived_users: string[];
  undelivered_notices: number;
  notices_given_up: number;
}

/**
 * The waiver is told by its moment, not by its author: `notices_waived_by` is blanked for
 * everyone but operators, while the stamp is public like every other moment on the change.
 * The count is read only while `scheduled` — the wire zeroes it elsewhere, and a state
 * check here keeps a figure that leaks through anyway from offering a button on a change
 * that is already active or cancelled.
 */
export function noticeSummary(change: NoticeSource): NoticeSummary {
  const waivedAt = Number(change.notices_waived_at);
  if ((Number.isFinite(waivedAt) && waivedAt > 0) || (change.notices_waived_by ?? "").trim()) {
    return { kind: "waived", by: change.notices_waived_by ?? "", at: change.notices_waived_at, holders: change.notices_waived_users.length };
  }
  if (change.state !== "scheduled" || change.undelivered_notices <= 0) return { kind: "none" };
  // The given-up notices are a part of the undelivered ones; a figure past that is a
  // wire fault and reads as "all of them", never as more holders than there are.
  const givenUp = Math.min(Math.max(change.notices_given_up, 0), change.undelivered_notices);
  if (givenUp === 0) return { kind: "queued", queued: change.undelivered_notices };
  return { kind: "givenUp", givenUp, queued: change.undelivered_notices - givenUp };
}
