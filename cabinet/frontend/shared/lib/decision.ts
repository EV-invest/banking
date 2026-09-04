// Reading a settled answer off the wire.
//
// This exists because "has this seat answered yet?" is not the question "is `decision`
// null?", and getting the two confused broke the feature completely. The money plane models
// an unanswered seat as an explicit `PENDING` state, and the BFF serialises that enum by
// name — so the field arrives as the string `"pending"`, and an absent one arrives as `""`
// rather than as JSON null. Both are truthy. A view that treats the field as nullable
// therefore reads *every fresh invitation* as already decided, and tells an owner who has
// just opened their email that they rejected a payout they have never seen.
//
// So the rule is inverted here, once, for every caller: an answer counts as settled only if
// it is one of the words that means an answer. Everything else — "pending", "", a state the
// money plane adds next year — is "not yet", which is the safe direction: the worst case is
// a vote form shown for a request the server will refuse, which produces a visible error,
// rather than a vote form silently withheld from someone entitled to use it.
//
// The removal vocabulary is doubled on purpose. The public route speaks approve/reject for
// both endpoints (the BFF maps them onto Remove/Keep), while the authenticated peer-vote
// route speaks remove/keep, and a removal's settled `decision` can reach the client through
// either. Accepting both spellings costs nothing and removes a whole class of "the button
// did nothing" bug.

/** An answer to a payout consilium, once one has actually been given. */
export type SettledPayout = "approve" | "reject";

/** An answer to a removal: `remove` ends the seat, `keep` refuses. */
export type SettledRemoval = "remove" | "keep";

const normalise = (raw: unknown): string => (typeof raw === "string" ? raw.trim().toLowerCase() : "");

/**
 * The payout answer this seat has given, or null while it is still open to them.
 *
 * Null means "no answer yet" and nothing else — never "the field was missing".
 */
export function settledPayout(raw: unknown): SettledPayout | null {
  const value = normalise(raw);
  if (value === "approve" || value === "approved") return "approve";
  if (value === "reject" || value === "rejected") return "reject";
  return null;
}

/** The removal answer, in whichever of the two vocabularies it arrived. */
export function settledRemoval(raw: unknown): SettledRemoval | null {
  const value = normalise(raw);
  if (value === "remove" || value === "removed" || value === "approve" || value === "approved") return "remove";
  if (value === "keep" || value === "kept" || value === "reject" || value === "rejected") return "keep";
  return null;
}

/**
 * One peer's vote, or null if they have not voted.
 *
 * Same shape as {@link settledRemoval} and deliberately the same function underneath: a peer
 * who has not answered arrives as `""`, and reading that as a cast vote both mislabels them
 * in the roster and takes the vote buttons away from someone who still has a vote to give.
 */
export function peerVote(raw: unknown): SettledRemoval | null {
  return settledRemoval(raw);
}

/**
 * An answer to an admission: `admit` grants the seat, `reject` refuses it.
 *
 * Its own vocabulary, and deliberately NOT {@link SettledRemoval}. The ownership plane
 * splits the two enums on purpose, and the words do not line up the way a reader would
 * assume: `reject` on a removal means *keep this owner*, `reject` on an admission means
 * *refuse this candidate*. Put an admission through `settledRemoval` and a rejection comes
 * back as `keep` — which renders "keep" over a candidate who has just been turned down,
 * and, worse, would let a shared vote control post the wrong verb to the wrong plane.
 */
export type SettledAdmission = "admit" | "reject";

/** The admission answer, or null while this seat has not answered. */
export function settledAdmission(raw: unknown): SettledAdmission | null {
  const value = normalise(raw);
  if (value === "admit" || value === "admitted") return "admit";
  if (value === "reject" || value === "rejected") return "reject";
  return null;
}

/**
 * One peer's vote on an admission, or null if they have not voted.
 *
 * The same trap as {@link peerVote}: an unanswered peer arrives as `""` or as the plane's
 * own `"pending"`, both truthy, and reading either as a cast vote takes the buttons away
 * from someone who still has a vote to give.
 */
export function admissionVote(raw: unknown): SettledAdmission | null {
  return settledAdmission(raw);
}
