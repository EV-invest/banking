-- Payments — the order that moves money between two named ends of the platform.
-- See `domain/src/payments.rs` for the aggregate and the reasoning this schema mirrors.
--
-- WHY AN ORDER AND NOT A SECOND SAGA. A payment is intent + approval + outcome. Executing
-- one produces EITHER a `withdrawals` row (the external tier, whose five-state saga and
-- "never void after a possible broadcast" rule stay entirely over there) OR one balanced
-- posted transfer in TigerBeetle. So this table never restates a withdrawal's lifecycle —
-- it records which effect happened and stops, exactly as `consilium` does.
--
-- WHY THERE IS NO `tier` COLUMN. The tier is a pure function of the destination: an address
-- is external, `service:<id>` is the service tier, anything else is internal. A column would
-- be a second statement of that fact, and the interesting failure is the one where the two
-- disagree — a row naming an address while claiming the internal tier. A projection that
-- does not exist cannot drift. The same argument retires `requirement` and `effect` columns:
-- the requirement is read off `from_kind` (fund-owned money needs the owner quorum, an
-- investor's claim needs that investor's consent) and the effect off the destination, so
-- both are derived at rehydrate rather than stored and re-derived.
--
-- WHY POSTGRES HOLDS NO BALANCE. `amount` is the exact base-unit count as TEXT, as every
-- other money column here; TigerBeetle stays the authoritative store of what a claim holds.

CREATE TABLE payments (
    id                     UUID PRIMARY KEY,
    state                  TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'rejected', 'expired', 'cancelled', 'executed', 'execution_failed')),

    -- The source claim, in the `(kind, id)` shape `Party::from_parts` reads and `deposits`
    -- already stores. TEXT rather than UUID because a service id is a slug, not a uuid.
    from_kind              TEXT NOT NULL CHECK (from_kind IN ('piggybank', 'user', 'service', 'revenue')),
    from_id                TEXT,
    -- The destination: EITHER an internal party (`to_kind`/`to_id`) OR an address on a rail
    -- (`to_network`/`to_address`). Never both, never neither — see `destination_is_coherent`.
    to_kind                TEXT CHECK (to_kind IN ('piggybank', 'user', 'service', 'revenue')),
    to_id                  TEXT,
    to_network             TEXT,
    to_address             TEXT,

    -- The same pattern `withdrawals.amount` uses, which also states "not zero": TigerBeetle
    -- rejects a zero-amount transfer, so an order for one is an approval spent on something
    -- that could never execute.
    amount                 TEXT NOT NULL CHECK (amount ~ '^[1-9][0-9]*$'),
    -- REQUIRED, and shown verbatim to whoever approves. Bounded in BYTES (matching
    -- `MAX_REASON_BYTES`) because that is what the column and the mail actually carry.
    reason                 TEXT NOT NULL CHECK (octet_length(reason) > 0 AND octet_length(reason) <= 500),
    -- SHA-256 over the CANONICAL encoding of the terms — `reason` included, so an approval
    -- binds to the sentence the human read and not merely to the amount.
    payload_hash           BYTEA NOT NULL CHECK (octet_length(payload_hash) = 32),
    initiator_user_id      UUID NOT NULL REFERENCES users (id),

    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at             TIMESTAMPTZ NOT NULL,
    decided_at             TIMESTAMPTZ,
    executed_withdrawal_id UUID REFERENCES withdrawals (id),
    failure_reason         TEXT,
    version                BIGINT NOT NULL DEFAULT 1 CHECK (version >= 1),

    -- The source user, derived by the DATABASE so it cannot disagree with `from_kind`/
    -- `from_id`. It exists only to carry the composite FK `payment_consent` points at (see
    -- that table): a generated column is the one kind of second copy that cannot drift.
    source_user_id         UUID GENERATED ALWAYS AS (CASE WHEN from_kind = 'user' THEN from_id::uuid END) STORED,
    -- Its mirror image for the other requirement: whether the source is the fund's money,
    -- derived by the DATABASE for the same reason. It exists only to carry the composite FK
    -- `payment_approval` points at, so an owner-quorum seat can only ever attach to an order
    -- the quorum actually decides.
    fund_owned             BOOLEAN GENERATED ALWAYS AS (from_kind <> 'user') STORED,

    -- EXACTLY ONE DESTINATION SHAPE. An internal party names a claim; an external one names
    -- a network AND an address. A row with both would have two answers to "what tier is
    -- this?", and a row with neither would have none.
    CONSTRAINT payments_destination_is_coherent CHECK (
        num_nonnulls(to_kind, to_network) = 1 AND (to_network IS NULL) = (to_address IS NULL) AND (to_kind IS NOT NULL OR to_id IS NULL)
    ),
    -- A claim paying itself moves nothing and would still consume an approval, an index slot
    -- and an owner's attention. `IS DISTINCT FROM` is what makes the singleton claims
    -- (`piggybank`, `revenue`, both id-less) compare equal to themselves here.
    CONSTRAINT payments_not_self CHECK (to_kind IS NULL OR NOT (from_kind = to_kind AND from_id IS NOT DISTINCT FROM to_id)),
    -- `pending` is exactly the un-decided state; every other state is a verdict.
    CONSTRAINT payments_verdict_matches_state CHECK ((state = 'pending') = (decided_at IS NULL)),
    -- THE EFFECT AND THE STATE ARE ONE FACT, and which effect is the destination's to say.
    -- An executed EXTERNAL order has a withdrawal; an executed internal one settled as a
    -- single posted transfer whose id is `uuid_v5(payment_id, "payment:transfer")` — a pure
    -- function of the row, so storing it would store a value that could only ever be wrong.
    CONSTRAINT payments_execution_is_recorded CHECK ((state = 'executed' AND to_kind IS NULL) = (executed_withdrawal_id IS NOT NULL)),
    -- A failure always says why; nothing else carries a reason.
    CONSTRAINT payments_failure_states_why CHECK ((state = 'execution_failed') = (failure_reason IS NOT NULL)),
    -- The composite target `payment_consent` references, so a consent seat can only ever
    -- name its own payment's actual source user.
    CONSTRAINT payments_id_source_user_key UNIQUE (id, source_user_id),
    -- The composite target `payment_approval` references, so a quorum seat can only ever
    -- name a payment whose source is actually the fund's.
    CONSTRAINT payments_id_fund_owned_key UNIQUE (id, fund_owned)
);

-- AT MOST ONE OPEN PAYMENT PER FUND-OWNED SOURCE CLAIM.
--
-- `pending` and `approved` are the two states in which an order still holds (or may yet
-- take) its source, so both are indexed. `NULLS NOT DISTINCT` is load-bearing: `piggybank`
-- and `revenue` carry no id, and under the default rule two open orders against `fund` would
-- both index as distinct NULLs and the invariant would silently not exist.
--
-- WHY `user` IS EXCLUDED, deliberately and not as an oversight. An investor's claim is
-- already covered twice over — `lock_claim` serialises every spender of it, and
-- TigerBeetle's `DebitsMustNotExceedCredits` flag is the last-line backstop — so a unique
-- index here would add no safety and would instead make one pending consent block that
-- investor's every other payment for up to 72h, which is a denial of service dressed as a
-- guarantee. The fund-owned claims get the index because their spenders (`consilium`'s own
-- per-source index, and this one) are the governance surface where a second open request is
-- a second quorum to chase rather than a queue to drain.
CREATE UNIQUE INDEX payments_single_open_per_source_idx ON payments (from_kind, from_id) NULLS NOT DISTINCT
    WHERE state IN ('pending', 'approved') AND from_kind <> 'user';

-- The expiry sweep's probe: the undecided rows past their deadline, and nothing else.
CREATE INDEX payments_open_expiry_idx ON payments (expires_at) WHERE state = 'pending';

-- The admin payments screen (`PaymentFeed::list`). Nothing is ever deleted — a rejected,
-- expired or failed order stays readable, because the audit record is half the point.
CREATE INDEX payments_history_idx ON payments (created_at DESC);

-- The activity-timeline branches: the payments an investor is on one END of. Two partial
-- indexes rather than one, because a user is a source or a destination and the feed's two
-- legs ask about the two columns separately.
CREATE INDEX payments_from_user_idx ON payments (from_id, created_at DESC) WHERE from_kind = 'user';
CREATE INDEX payments_to_user_idx ON payments (to_id, created_at DESC) WHERE to_kind = 'user';

-- The owner-quorum requirement, MATERIALIZED. One row exists exactly for the payments whose
-- source is fund-owned, written in the same transaction as the order itself — so a payment
-- can never exist with no record of what would authorize it, and the authorization can never
-- exist without its payment.
--
-- WHY A LINK TABLE AND NOT A COLUMN ON EITHER SIDE. `consilium.executed_payment_id` answers
-- a different question: what an approved consilium PRODUCED, written at execution. This row
-- answers what an order REQUIRES, written at open. Folding them into one column would mean a
-- pending payment and an executed one are told apart by a nullable FK, which is precisely the
-- ambiguity `consilium_execution_is_recorded` exists to remove.
--
-- Both sides are unique: a payment has one consilium, and a consilium authorizes one payment.
-- `ON DELETE RESTRICT` on the consilium end because a governance record is never deleted.
--
-- WHY THE COMPOSITE FK, the mirror of `payment_consent`'s. §3 says fund-owned money is
-- decided by the owner quorum and an investor's claim by that investor alone; the consent
-- table below makes the second half unrepresentable otherwise, and this makes the first. A
-- user-sourced order generates `fund_owned = FALSE`, so the `(id, TRUE)` pair this row must
-- reference does not exist for it — an owner quorum cannot be seated over an investor's
-- money by any code path, forgotten check or not.
CREATE TABLE payment_approval (
    payment_id   UUID PRIMARY KEY REFERENCES payments (id) ON DELETE CASCADE,
    consilium_id UUID NOT NULL UNIQUE REFERENCES consilium (id) ON DELETE RESTRICT,
    -- Always TRUE, and only ever TRUE: the column is the FK's second key and nothing else.
    fund_owned   BOOLEAN NOT NULL DEFAULT TRUE CHECK (fund_owned),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (payment_id, fund_owned) REFERENCES payments (id, fund_owned) ON DELETE CASCADE
);

-- The investor-consent requirement, MATERIALIZED — one seat, and the credentials mailed to it.
--
-- WHY NOT `consilium` WITH N = 1. It would require relaxing `consilium_quorum_is_reachable`
-- (`owner_count >= 3 AND threshold >= 2`) and `consilium_voter_is_not_the_initiator` — and a
-- user-initiated payment is exactly the case where the subject IS the initiator. Weakening the
-- constraints that protect a quorum in order to express a non-quorum is a bad trade. What IS
-- shared is the SPECIFICATION: the two CHECKs below are `consilium_voter`'s, copied word for
-- word, and `docs/CONSILIUM.md` § "One specification for both planes" is the contract the
-- tests assert against for both.
--
-- WHY THE COMPOSITE FK. `(payment_id, subject_user_id)` points at `payments (id,
-- source_user_id)`, whose second column the database GENERATES from `from_kind`/`from_id`.
-- So "an investor's payment is consented to by that investor and nobody else" — the §3 rule —
-- is unrepresentable otherwise, rather than a check some future code path could forget.
CREATE TABLE payment_consent (
    payment_id                    UUID PRIMARY KEY REFERENCES payments (id) ON DELETE CASCADE,
    subject_user_id               UUID        NOT NULL REFERENCES users (id),
    decision                      TEXT        NOT NULL DEFAULT 'pending' CHECK (decision IN ('pending', 'approve', 'reject')),
    decided_at                    TIMESTAMPTZ,
    -- True once the consent mail has actually been handed to the delivery queue.
    notified                      BOOLEAN     NOT NULL DEFAULT FALSE,
    -- SHA-256 digests only. The plaintexts live in the mail row until the message is handed
    -- to the mailer, so a dump of this table yields nothing that can consent to anything.
    token_hash                    BYTEA       NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    code_hash                     BYTEA       NOT NULL CHECK (octet_length(code_hash) = 32),
    -- Incremented in the SAME transaction as (and BEFORE) the code comparison, so concurrent
    -- guesses cannot slip past the limit. At 5 the token burns — and burning FAILS THE PAYMENT
    -- CLOSED: with one seat there is no second party to escalate to, so it is a refusal rather
    -- than a detector.
    attempts                      INTEGER     NOT NULL DEFAULT 0 CHECK (attempts >= 0 AND attempts <= 5),
    burned_at                     TIMESTAMPTZ,
    expires_at                    TIMESTAMPTZ NOT NULL,
    used_at                       TIMESTAMPTZ,
    -- What the subject PERSONALLY signed, so an auditor can prove the terms shown at the
    -- moment of consent are the terms that were paid.
    payload_hash                  BYTEA       CHECK (payload_hash IS NULL OR octet_length(payload_hash) = 32),

    -- THE TWO PINS RE-CHECKED AT CONSENT AND AGAIN AT EXECUTION, fail-closed. Acceptance and
    -- execution can be 72h apart and the consent surface is mounted outside the auth layer, so
    -- each records what was true at open; a pin that has moved rejects a pending order at
    -- consent and fails an approved one at execution:
    --   - the token version (the folded revoke floor, `GREATEST(concierge_token_version,
    --     token_version)`) is what makes `RevokeTokens` on either plane void a live consent;
    --   - the email hash (SHA-256 over `users.email` as stored) is what stops a mailbox change
    --     at the identity provider redirecting a token that is already in flight.
    subject_token_version_at_open BIGINT      NOT NULL CHECK (subject_token_version_at_open >= 0),
    subject_email_hash_at_open    BYTEA       NOT NULL CHECK (octet_length(subject_email_hash_at_open) = 32),

    -- Who consented, from where, with what. Audit only: the edge supplies these and they are
    -- never trusted for authorization.
    client_ip                     TEXT,
    user_agent                    TEXT,

    FOREIGN KEY (payment_id, subject_user_id) REFERENCES payments (id, source_user_id) ON DELETE CASCADE,
    -- Copied verbatim from `consilium_voter_decision_is_atomic`: a seat has answered exactly
    -- when it has a timestamp and has spent its token, so a half-recorded decision is
    -- unrepresentable.
    CONSTRAINT payment_consent_decision_is_atomic CHECK ((decision = 'pending') = (decided_at IS NULL) AND (decision = 'pending') = (used_at IS NULL)),
    -- Copied verbatim from `consilium_voter_burn_is_exhaustion`: burning is terminal and only
    -- ever happens at the attempt ceiling.
    CONSTRAINT payment_consent_burn_is_exhaustion CHECK ((burned_at IS NULL) OR (attempts >= 5))
);

-- WIRE UP `consilium.executed_payment_id`. 0029 added the column with no FK because the table
-- it names did not exist yet; now it does. The column is NULL on every row (no `kind` can
-- write it yet), so the constraint validates instantly and locks nothing measurable.
--
-- `consilium.kind` IS DELIBERATELY NOT WIDENED HERE. 0029 states the rule: the CHECK gains
-- `'payment'` in the migration that introduces `ConsiliumKind::Payment` in Rust, in the same
-- commit, because a row this binary cannot LOAD fails every read of the governance history and
-- not merely its own. Until then `payment_approval` is an empty table whose FK target cannot
-- be produced — which is the correct state for a shape that has landed ahead of its writer.
ALTER TABLE consilium ADD CONSTRAINT consilium_executed_payment_fk FOREIGN KEY (executed_payment_id) REFERENCES payments (id);
