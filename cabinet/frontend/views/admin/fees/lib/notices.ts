// What a change's notice figures say — four things, and the order they are read in.
//
// Import-free like `./receipt.ts`, so `node --test` can exercise the decisions that
// matter. The hub counts the given-up notices its acknowledgement does NOT cover
// (`notices_unacknowledged` — all of them while there is none), and that figure, not the
// waiver, decides whether the act is offered: an acknowledgement covers exactly the
// holders given up on by its moment, and a holder the mailer gives up on afterwards is
// as untold as the first ones were. A later act adds them to the record and puts its
// caller's name on the whole list. Only once nothing is left uncovered does the waiver
// stand alone as history. And a notice still in the mailer's queue is not one the holder
// "could not be told" — right after scheduling every notice is undelivered for the
// half-minute before the first attempt, and the hub refuses an acknowledgement until it
// has given up on at least one.

/** Somebody took responsibility: who, when (unix seconds as a string), and over how many. */
export interface Waiver {
  /** The operator's user id — the fact on the record; `""` when blanked for the reader. */
  by: string;
  /** The same operator by email, `null` when the directory could not name the id. */
  email: string | null;
  at: string;
  holders: number;
}

/**
 * The name a reader is shown for the waiver's author: the email when the directory knows
 * one, the id otherwise — a UUID is a poor name but a true one, and hiding it would read as
 * an anonymous act. `null` when the author is blanked for the reader.
 */
export function waiverName(waiver: Pick<Waiver, "by" | "email">): string | null {
  return waiver.email?.trim() || waiver.by.trim() || null;
}

export type NoticeSummary =
  /** Nothing to say: every notice delivered, or a state in which none are counted. */
  | { kind: "none" }
  /** Notices the mailer is still trying to deliver, none given up on: nothing to do yet. */
  | { kind: "queued"; queued: number }
  /** Holders the mailer has given up on that no acknowledgement covers — the ones the act
   *  would name — how many notices beside theirs are still being tried, and the record an
   *  act would extend (`null` for a first one). */
  | { kind: "givenUp"; givenUp: number; queued: number; waiver: Waiver | null }
  /** Somebody took responsibility and nothing has been given up on since. */
  | ({ kind: "waived" } & Waiver);

export interface NoticeSource {
  state: string;
  notices_waived_by: string | null;
  notices_waived_by_email: string | null;
  notices_waived_at: string;
  notices_waived_users: string[];
  undelivered_notices: number;
  notices_given_up: number;
  notices_unacknowledged: number;
}

/**
 * The acknowledgement on record, whatever has been given up on since — history, so it
 * reads in every state. It is told by its moment, not by its author: `notices_waived_by`
 * is blanked for everyone but operators, while the stamp is public like every other
 * moment on the change.
 */
export function waiverRecord(change: NoticeSource): Waiver | null {
  const waivedAt = Number(change.notices_waived_at);
  if ((Number.isFinite(waivedAt) && waivedAt > 0) || (change.notices_waived_by ?? "").trim()) {
    return { by: change.notices_waived_by ?? "", email: change.notices_waived_by_email, at: change.notices_waived_at, holders: change.notices_waived_users.length };
  }
  return null;
}

/**
 * The counts are read only while `scheduled` — the wire zeroes them elsewhere, and a
 * state check here keeps a figure that leaks through anyway from offering a button on a
 * change that is already active or cancelled.
 */
export function noticeSummary(change: NoticeSource): NoticeSummary {
  const waiver = waiverRecord(change);
  const waived = (): NoticeSummary => (waiver ? { kind: "waived", ...waiver } : { kind: "none" });
  if (change.state !== "scheduled" || change.undelivered_notices <= 0) return waived();
  // The given-up notices are a part of the undelivered ones, and the uncovered ones a part
  // of the given-up; a figure past its whole is a wire fault and reads as "all of them",
  // never as more holders than there are.
  const givenUp = Math.min(Math.max(change.notices_given_up, 0), change.undelivered_notices);
  const uncovered = Math.min(Math.max(change.notices_unacknowledged, 0), givenUp);
  const queued = change.undelivered_notices - givenUp;
  if (uncovered > 0) return { kind: "givenUp", givenUp: uncovered, queued, waiver };
  if (waiver) return waived();
  return queued > 0 ? { kind: "queued", queued } : { kind: "none" };
}
