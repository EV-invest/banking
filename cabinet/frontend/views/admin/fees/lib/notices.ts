// What a change's notice figures say — three things, and the order they are read in.
//
// Import-free like `./receipt.ts`, so `node --test` can exercise the one decision that
// matters: a waiver outranks the count. After an acknowledgement the hub keeps reporting
// the undelivered notices (they are still undelivered), and a card that read the count
// first would offer to take responsibility for holders somebody already took it for.

export type NoticeSummary =
  /** Nothing to say: every notice delivered, or a state in which none are counted. */
  | { kind: "none" }
  /** Holders a scheduled change has not reached, and how many the mailer has given up on. */
  | { kind: "undelivered"; undelivered: number; givenUp: number }
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
  if (change.state === "scheduled" && change.undelivered_notices > 0) {
    return { kind: "undelivered", undelivered: change.undelivered_notices, givenUp: Math.min(change.notices_given_up, change.undelivered_notices) };
  }
  return { kind: "none" };
}
