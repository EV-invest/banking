-- Consilium — the multi-owner authorization gate in front of the fund paying its OWN
-- earned revenue out. See docs/CONSILIUM.md for the policy and the whole threat model.
--
-- WHY A SEPARATE AGGREGATE, NOT A WITHDRAWAL STATE. A consilium is an *authorization
-- artifact*, not a money move: no claim is reserved, nothing is queued, nothing can be
-- refunded. Folding it into `withdrawals` would put a 72h human-latency state in front of
-- the reserve, which is what makes the withdrawal saga's cardinal rule ("never void once
-- the broadcast may have landed") reasonable to reason about. So the consilium sits
-- entirely beside the money aggregate and, on approval, opens an ordinary revenue payout
-- through the existing path. The blast radius stays off the money plane.
--
-- WHY THE ROSTER IS SNAPSHOTTED HERE. `threshold` and `owner_count` are frozen at open
-- and never recomputed. Changing the roster afterwards can therefore only make approval
-- HARDER (a removed owner's vote is voided at tally; a new owner gets no token), never
-- easier — closing the quorum-lowering and roster-stuffing holes in one stroke.
--
-- WHY ONLY ONE MAY BE OPEN. Two approved payouts racing to execute could between them
-- overdraw the `fee` claim; the partial unique index below removes that race rather than
-- trying to win it. Insufficient revenue at execution is still handled — the payout path's
-- own Read-First refuses and the consilium lands in `execution_failed`.

CREATE TABLE consilium (
    id                     UUID PRIMARY KEY,
    -- Only one kind today. Named rather than implied so a second governance subject
    -- (should one ever land in the money plane) is an enum value, not a new table.
    kind                   TEXT        NOT NULL CHECK (kind IN ('revenue_payout')),
    state                  TEXT        NOT NULL CHECK (state IN ('open', 'approved', 'rejected', 'expired', 'cancelled', 'executed', 'execution_failed')),
    -- The immutable subject: {network, address, amount, memo}. There is no edit path —
    -- changing anything means cancel and reopen, and votes are not carried over.
    terms                  JSONB       NOT NULL,
    -- SHA-256 over the CANONICAL encoding of `terms` (fixed field order, length-prefixed),
    -- stored at open and re-verified before execution. This is what binds an owner's
    -- approval to the exact terms their mail showed them.
    payload_hash           BYTEA       NOT NULL CHECK (octet_length(payload_hash) = 32),
    initiator_user_id      UUID        NOT NULL REFERENCES users (id),
    owner_count            INTEGER     NOT NULL,
    threshold              INTEGER     NOT NULL,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at             TIMESTAMPTZ NOT NULL,
    decided_at             TIMESTAMPTZ,
    executed_withdrawal_id UUID        REFERENCES withdrawals (id),
    failure_reason         TEXT,
    version                BIGINT      NOT NULL DEFAULT 1 CHECK (version >= 1),

    -- A fund below three owners can never reach `floor(N/2)+1` with the initiator barred
    -- from voting, so such a request is refused at open rather than stored as one that can
    -- never pass. The database states the floor too, so no future caller can bypass it.
    CONSTRAINT consilium_quorum_is_reachable CHECK (owner_count >= 3 AND threshold >= 2 AND threshold <= owner_count),
    -- `open` is exactly the un-decided state; every other state is a verdict.
    CONSTRAINT consilium_verdict_matches_state CHECK ((state = 'open') = (decided_at IS NULL)),
    -- The payout id is written exactly once, on `executed` — the column and the state are
    -- one fact, so a row cannot claim a payout it never made (or hide one it did).
    CONSTRAINT consilium_execution_is_recorded CHECK ((state = 'executed') = (executed_withdrawal_id IS NOT NULL)),
    -- A failure always says why; nothing else carries a reason.
    CONSTRAINT consilium_failure_states_why CHECK ((state = 'execution_failed') = (failure_reason IS NOT NULL)),
    -- The composite target the voter table's FK points at, so a voter row can only ever
    -- name its own consilium's real initiator (see `consilium_voter` below).
    CONSTRAINT consilium_id_initiator_key UNIQUE (id, initiator_user_id)
);

-- AT MOST ONE OPEN CONSILIUM, ENFORCED BY THE DATABASE. A unique index on a constant
-- expression, restricted to the open rows: every open row indexes the same key, so a
-- second one cannot be inserted. This is the whole of the concurrent-approval overdraw
-- defence — a race that cannot be created does not have to be won.
CREATE UNIQUE INDEX consilium_single_open_idx ON consilium ((TRUE)) WHERE state = 'open';

-- The expiry sweep's probe: the open rows past their deadline, and nothing else.
CREATE INDEX consilium_open_expiry_idx ON consilium (expires_at) WHERE state = 'open';

-- The governance history screen. Nothing is ever deleted — a rejected, expired or failed
-- consilium stays readable, because the audit record is half the point of the feature.
CREATE INDEX consilium_history_idx ON consilium (created_at DESC);

-- One eligible owner's seat, and the credentials mailed to it.
--
-- WHY ONLY HASHES. `token_hash` and `code_hash` are SHA-256 digests; the plaintexts exist
-- only in the mail queue row below, until the message is handed to the mailer. A dump of
-- this table therefore yields nothing that can cast a vote.
--
-- WHY THE INITIATOR CANNOT HAVE A ROW. The composite FK forces `initiator_user_id` to be
-- this consilium's actual initiator, and the CHECK then forbids `user_id` from equalling
-- it. So the "the initiator gets no vote" rule is not a check some future code path could
-- forget to run — there is no row for them, hence no token, hence nothing to submit.
CREATE TABLE consilium_voter (
    consilium_id      UUID        NOT NULL REFERENCES consilium (id) ON DELETE CASCADE,
    user_id           UUID        NOT NULL REFERENCES users (id),
    -- Denormalised solely to carry the composite FK below. Never read as data.
    initiator_user_id UUID        NOT NULL,
    decision          TEXT        NOT NULL DEFAULT 'pending' CHECK (decision IN ('pending', 'approve', 'reject')),
    decided_at        TIMESTAMPTZ,
    -- True once the approval mail has actually been handed to the delivery queue.
    notified          BOOLEAN     NOT NULL DEFAULT FALSE,
    token_hash        BYTEA       NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    code_hash         BYTEA       NOT NULL CHECK (octet_length(code_hash) = 32),
    -- Incremented in the SAME transaction as (and BEFORE) the code comparison, so
    -- concurrent guesses cannot slip past the limit. At 5 the token burns.
    attempts          INTEGER     NOT NULL DEFAULT 0 CHECK (attempts >= 0 AND attempts <= 5),
    burned_at         TIMESTAMPTZ,
    expires_at        TIMESTAMPTZ NOT NULL,
    used_at           TIMESTAMPTZ,
    -- Who voted, from where, with what. Audit only: the edge supplies these and they are
    -- never trusted for authorization, but a payout nobody can account for afterwards is
    -- exactly the failure this whole feature exists to prevent.
    client_ip         TEXT,
    user_agent        TEXT,

    PRIMARY KEY (consilium_id, user_id),
    FOREIGN KEY (consilium_id, initiator_user_id) REFERENCES consilium (id, initiator_user_id) ON DELETE CASCADE,
    CONSTRAINT consilium_voter_is_not_the_initiator CHECK (user_id <> initiator_user_id),
    -- A seat has answered exactly when it has a timestamp and has spent its token; the
    -- three columns are one fact, so a half-recorded vote is unrepresentable.
    CONSTRAINT consilium_voter_decision_is_atomic CHECK ((decision = 'pending') = (decided_at IS NULL) AND (decision = 'pending') = (used_at IS NULL)),
    -- Burning is terminal and only ever happens at the attempt ceiling.
    CONSTRAINT consilium_voter_burn_is_exhaustion CHECK ((burned_at IS NULL) OR (attempts >= 5))
);

-- The mail queue.
--
-- WHY A QUEUE AND NOT AN INLINE CALL. The mail is enqueued in the SAME transaction as the
-- consilium fact it announces, and delivered afterwards by a worker. A concierge outage
-- must never roll back a vote that was validly cast, and a vote must never be recorded
-- without its notification eventually going out. Writing both in one transaction and
-- draining the second half asynchronously is the only shape that gives both.
--
-- WHY THE PLAINTEXT CODE LIVES HERE. The owner cannot type a hash, so the secret has to
-- survive until the message is rendered. It lives in exactly one row, for as long as it
-- takes the worker to hand it over, and `payload` is rewritten without it on success —
-- mirroring what concierge does with its own delivery rows.
CREATE TABLE consilium_mail (
    id           BIGSERIAL PRIMARY KEY,
    consilium_id UUID        NOT NULL REFERENCES consilium (id) ON DELETE CASCADE,
    -- The BANKING user id of the recipient. The adapter resolves it to the concierge id
    -- (`users.concierge_user_id`) at send time, because the relay RPC addresses identities
    -- in the plane that owns them — the money plane cannot redirect a governance mail.
    user_id      UUID        NOT NULL REFERENCES users (id),
    kind         TEXT        NOT NULL CHECK (kind IN ('payout_approval', 'payout_outcome', 'token_burned')),
    -- The relay RPC is idempotent by this key, so a redelivering worker never double-sends.
    dedupe_key   TEXT        NOT NULL UNIQUE,
    payload      JSONB       NOT NULL,
    attempts     INTEGER     NOT NULL DEFAULT 0,
    last_error   TEXT,
    sent_at      TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The worker's drain probe: the unsent rows, oldest first. Sent rows stay for the audit
-- trail but leave the index.
CREATE INDEX consilium_mail_pending_idx ON consilium_mail (id) WHERE sent_at IS NULL;
