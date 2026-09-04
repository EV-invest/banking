// The fund-governance surface: the owner roster, owner removals, the payout consilium, and
// the two token-addressed approval endpoints an owner reaches from their mailbox.
//
// Hand-written rather than generated, like `./notifications` and `./admin`: these are BFF
// DTOs shaped for this cabinet, not a direct projection of a banking proto message (the
// removal half is concierge-owned, the payout half banking-owned, and the BFF joins them —
// see docs/CONSILIUM.md § The two planes). When the surface does get a proto, this file is
// what `./gen` replaces.
//
// Two rules from the policy are encoded in the *types* rather than left to a view:
//
//   · A vote is a decision, not a number. The public GET carries `attempts_remaining` and a
//     settled `decision`, never a running tally the client could add to.
//   · A socket frame carries a REVISION and nothing else (see the store in
//     `entities/governance/model/consilium-socket.ts`). There is no tally on the wire, so
//     there is no type here that could accidentally be rendered as one.
//
// Lifecycle states travel as plain strings on purpose. The state machine lives in the money
// plane and gains members there (`ExecutionFailed` arrived after `Approved`); a closed union
// here would turn a backend addition into a client type error and, worse, tempt a view into
// an exhaustive switch that renders nothing at all for a state it has not met yet. Views
// label the states they know and fall back to the wire value, which is legible even when it
// is new.

/** An amount as it crossed the wire: an exact decimal string, never a number. */
export type Decimal = string;

/** RFC 3339, as every other timestamp in this cabinet. */
export type Timestamp = string;

// ── The payout being authorized ────────────────────────────────────────────────

/**
 * The money movement a payout consilium authorizes, and the whole of what an owner is
 * agreeing to. Every field is shown in full on the approval page: `payload_hash` is
 * computed over exactly this and re-verified at execution, so anything hidden from the
 * reader is something they approved without seeing (policy 12–13).
 */
export interface RevenuePayout {
  network: string;
  /** Rendered in full, monospace, never truncated. See policy 13. */
  address: string;
  amount: Decimal;
  memo?: string | null;
}

// ── Public: the emailed payout approval ────────────────────────────────────────

export type PayoutDecision = "approve" | "reject";

/**
 * What `GET /api/approval/payout/:token` renders — a redacted summary and nothing else.
 * The GET is strictly read-only because mail scanners click every link in a message
 * (policy 5); the vote is the POST below, carrying the secret code.
 *
 * There is no "which failure was it" field, and there must never be one: unknown, expired,
 * spent and burned tokens all answer with one identical 404 (policy 10).
 */
export interface PayoutApproval {
  consilium_id: string;
  state: string;
  revenue_payout: RevenuePayout;
  /** Full hash; the page shows a short prefix of it. */
  payload_hash: string;
  initiator_email: string;
  /** The seat this token was minted for — the "you" the page addresses. */
  voter_email: string;
  threshold: number;
  approvals: number;
  owner_count: number;
  created_at: Timestamp;
  expires_at: Timestamp;
  /**
   * This seat's answer as the wire spells it — NOT a nullable `PayoutDecision`.
   *
   * An unanswered seat is an explicit `PENDING` in the money plane and reaches the client
   * as the string `"pending"`; an absent one reaches it as `""`. Both are truthy, so this
   * must go through `settledPayout` (`shared/lib/decision.ts`) rather than a `?? null`.
   * Typing it as the narrow union is what made every fresh invitation look decided.
   */
  decision?: string | null;
  /** Wrong codes left before the token burns and every owner is notified (policy 7). */
  attempts_remaining: number;
}

/**
 * The answer to a cast vote. `invitation` is the authoritative re-read — the view renders
 * that, never its own optimistic guess — and `decided` says whether this POST is what
 * settled the seat.
 */
export interface PayoutApprovalResult {
  invitation: PayoutApproval;
  decided: boolean;
}

// ── Public: the emailed "your seat is being removed" notice ────────────────────

export type RemovalDecision = "remove" | "keep";

/** What `GET /api/approval/removal/:token` renders. Same contract shape, different content. */
export interface RemovalApproval {
  removal_id: string;
  state: string;
  initiator_email: string;
  /** The owner whose seat this is about — the reader. */
  target_email: string;
  reason: string;
  created_at: Timestamp;
  expires_at: Timestamp;
  /** As the wire spells it — read through `settledRemoval`. See {@link PayoutApproval.decision}. */
  decision?: string | null;
  attempts_remaining: number;
}

export interface RemovalApprovalResult {
  invitation: RemovalApproval;
  decided: boolean;
}

// ── Authenticated: the owners' room ────────────────────────────────────────────

export interface Owner {
  user_id: string;
  email: string;
  display_name: string;
  owner_since: Timestamp;
}

/**
 * The roster, plus the one derived fact the page has to lead with.
 *
 * `below_payout_floor` is computed by the plane that owns the rule, not re-derived here
 * from `items.length`. A fund under three owners can never authorize a payout again
 * (policy: quorum arithmetic), and that is the sort of arithmetic a client must read
 * rather than reimplement.
 */
export interface OwnerList {
  items: Owner[];
  below_payout_floor: boolean;
}

export type RemovalVote = "remove" | "keep";

/**
 * One eligible peer voter and their answer.
 *
 * A peer who has not voted arrives with `vote` as an empty string, not as null or absent —
 * so this is the wire's spelling and must go through `peerVote` (`shared/lib/decision.ts`).
 * Reading `""` as a cast vote both mislabels them in the roster and removes the vote
 * buttons from someone who still has a vote to give.
 */
export interface RemovalPeer {
  user_id: string;
  email: string;
  vote?: string | null;
  voted_at?: Timestamp | null;
}

/**
 * An open (or settled) proposal to remove an owner.
 *
 * `peers` is the snapshot of who may vote, frozen when the removal was opened — the
 * initiator and the target are both absent from it by construction, so membership of this
 * array IS the eligibility test. A view must not re-derive eligibility from the roster:
 * an owner added since the removal opened is not a voter (policy 3), and the roster does
 * not record that.
 */
export interface OwnerRemoval {
  id: string;
  state: string;
  target_user_id: string;
  target_email: string;
  initiator_user_id: string;
  initiator_email: string;
  reason: string;
  peers: RemovalPeer[];
  /** The target's own answer from their mailbox — the other path to a decision. Wire
   *  spelling; read through `settledRemoval`. */
  target_decision?: string | null;
  target_decided_at?: Timestamp | null;
  target_notified: boolean;
  owner_count: number;
  created_at: Timestamp;
  expires_at: Timestamp;
  decided_at?: Timestamp | null;
  void_reason?: string | null;
  /** Serialised as a string by the BFF's integer encoding; never do arithmetic on it here. */
  version: number | string;
}

export interface OwnerRemovalList {
  items: OwnerRemoval[];
}

/**
 * An answer to an admission. Its own union, not a reuse of {@link RemovalVote}, mirroring
 * the plane's own split — `reject` here refuses a candidate, while `reject` on a removal
 * keeps an owner. A single shared type would make posting the wrong verb a type-correct
 * mistake, and this is the one surface where that mistake seats or unseats someone.
 */
export type AdmissionVote = "admit" | "reject";

/**
 * One eligible peer voter on an admission and their answer.
 *
 * Same wire caveat as {@link RemovalPeer}: an unanswered peer arrives as `""` or
 * `"pending"`, so `vote` goes through `admissionVote` (`shared/lib/decision.ts`), never a
 * `?? null`.
 */
export interface AdmissionPeer {
  user_id: string;
  email: string;
  vote?: string | null;
  voted_at?: Timestamp | null;
}

/**
 * A proposal to grant someone a seat — the ONLY way `Role::Owner` is granted once the fund
 * holds two owners (`SetRole` refuses it outside an executed admission).
 *
 * Two differences from {@link OwnerRemoval}, and neither is cosmetic:
 *
 *   · There is no target-decision trio. The candidate is not an owner yet, so they have no
 *     say in their own admission and there is no mailbox path that could carry it.
 *   · `peers` is every owner except the initiator, and admission passes only on UNANIMITY
 *     of that set with at least one member in it. Removal has a second path (the target's
 *     own acceptance); admission has none, so an absent peer is a proposal that cannot
 *     pass rather than one that passes another way.
 *
 * As with removals, membership of `peers` IS the eligibility test — it is the voter set
 * frozen at open, and an owner seated since is not in it (docs/CONSILIUM.md, policy 3).
 */
export interface OwnerAdmission {
  id: string;
  state: string;
  candidate_user_id: string;
  candidate_email: string;
  initiator_user_id: string;
  initiator_email: string;
  reason: string;
  peers: AdmissionPeer[];
  owner_count: number;
  created_at: Timestamp;
  expires_at: Timestamp;
  decided_at?: Timestamp | null;
  void_reason?: string | null;
  /** Serialised as a string by the BFF's integer encoding; never do arithmetic on it here. */
  version: number | string;
}

export interface OwnerAdmissionList {
  items: OwnerAdmission[];
}

/**
 * A consilium as the owners' room sees it. Only the payout kind exists today, so
 * `revenue_payout` is what distinguishes it; a second kind would arrive as a sibling field
 * rather than by widening this one.
 *
 * The fields past `expires_at` are the ones the money plane records as a request settles.
 * They are optional because an open request carries none of them, and because a client
 * that hard-requires an outcome field cannot render a request that has no outcome yet.
 */
export interface Consilium {
  id: string;
  state: string;
  revenue_payout?: RevenuePayout | null;
  payload_hash: string;
  initiator_user_id?: string;
  initiator_email: string;
  threshold: number;
  approvals: number;
  owner_count: number;
  created_at: Timestamp;
  expires_at: Timestamp;
  decided_at?: Timestamp | null;
  /** Why a request that reached `Approved` did not move money (policy 16). */
  failure_reason?: string | null;
  executed_withdrawal_id?: string | null;
}

export interface ConsiliumList {
  items: Consilium[];
}

// ── The realtime frame ─────────────────────────────────────────────────────────

/**
 * Everything the websocket is allowed to say.
 *
 * A frame is a doorbell, not a delivery: `revision` moves, the client re-reads the
 * authoritative snapshot over REST, and a stale or spoofed frame can therefore only ever
 * cause a redundant fetch — never a wrong count on screen (policy 21). A `heartbeat` frame
 * proves the connection is alive and carries no revision change.
 */
export interface ConsiliumFrame {
  revision?: number;
  at?: Timestamp;
  heartbeat?: boolean;
}
