// Reading the three refusals that stop a payout consilium being opened.
//
// The money plane raises all three as `DomainError::Conflict`, which reaches the browser as
// one status with the plane's own prose in the body. `shared/lib/api-client.ts` passes prose
// it did not author through unkeyed and in whatever language the backend chose — correct as
// a default, and not good enough here: these are the three sentences an operator meets when
// the fund's own payout is refused, and each one has a different thing they should do next.
//
// So the message is classified rather than echoed, and what the classifier extracts is the
// FACTS inside it — how long the cooling-off has left, how many owners the fund has — so the
// screen can render them in the reader's language and, in the cooling-off case, as a real
// clock time rather than a duration that was already going stale when it arrived.
//
// Matching on prose is a seam, and it is a load-bearing one. It fails safe: an unrecognised
// message classifies as `null` and the caller shows the backend's own words, which is
// exactly what happened before this module existed. So a reworded backend string degrades
// to the previous behaviour rather than to a blank or a wrong explanation. The matches are
// deliberately anchored on the invariant part of each sentence (the fragment that names the
// condition) rather than on the whole string, which is the part most likely to be reworded.

/** A refusal the screen has something specific to say about. */
export type ConsiliumRefusal =
  /**
   * No mailer is wired, so no owner could be sent an approval token. A consilium opened now
   * could never be voted on, which is why the plane refuses rather than opening a dead one.
   */
  | { kind: "mail-not-configured" }
  /**
   * The owner roster changed within the last 48 hours and payouts are paused until it
   * settles. The delay exists to make a roster seizure and a payout two visible events
   * rather than one motion (docs/CONSILIUM.md).
   */
  | { kind: "cooling-off"; hours: number; minutes: number }
  /** Below three owners the threshold is arithmetically unreachable. */
  | { kind: "too-few-owners"; ownerCount: number | null };

/** The message carried by whatever the transport threw, or "" if it carried none. */
function messageOf(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "";
}

/**
 * Classify a failed `POST /api/consilium/revenue-payout`.
 *
 * Returns null for anything not recognised — including every ordinary failure (offline, a
 * 500, a bad amount). The caller renders those the way it always has.
 */
export function classifyConsiliumRefusal(error: unknown): ConsiliumRefusal | null {
  const message = messageOf(error).toLowerCase();
  if (!message) return null;

  if (message.includes("governance mail is not configured")) return { kind: "mail-not-configured" };

  if (message.includes("cooling-off")) {
    // "…lifts in 12h 30m". Both parts are present in every emission, but the parse is
    // tolerant: a message that has changed shape still classifies as a cooling-off, and the
    // caller falls back to naming the condition without a time rather than showing "NaN".
    const found = /lifts in\s+(\d+)h\s*(\d+)m/.exec(message);
    return {
      kind: "cooling-off",
      hours: found ? Number(found[1]) : 0,
      minutes: found ? Number(found[2]) : 0,
    };
  }

  if (message.includes("needs at least") && message.includes("owners")) {
    const found = /this fund has\s+(\d+)/.exec(message);
    return { kind: "too-few-owners", ownerCount: found ? Number(found[1]) : null };
  }

  return null;
}

/**
 * When the cooling-off lifts, as an absolute moment.
 *
 * Taken once, at the instant the refusal arrives, and never recomputed: the backend sent a
 * duration, and a duration re-based on every render would silently drift later and later
 * away from the deadline it describes. An absolute time is also the more useful of the two
 * to an operator — "resumes at 14:20 tomorrow" is a thing you can plan around, "in 12h 30m"
 * is a thing you have to do arithmetic on.
 */
export function coolingOffLiftsAt(refusal: { hours: number; minutes: number }, now: number = Date.now()): string {
  return new Date(now + refusal.hours * 3_600_000 + refusal.minutes * 60_000).toISOString();
}
