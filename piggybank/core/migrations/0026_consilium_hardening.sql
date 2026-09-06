-- Consilium hardening: the audit column the spec already promised, the index the sweeper
-- needs, and the roster-change record a cooling-off period is measured from.

-- 1. WHAT EACH VOTE WAS CAST AGAINST.
--
-- docs/CONSILIUM.md states that a vote records the `payload_hash` it was cast against, and
-- until now no such column existed — the claim was true of the consilium but not of the
-- individual approval. It matters: `execute` re-verifies the hash over the whole request, so
-- an edited row cannot spend the approvals, but nothing recorded WHICH terms each owner
-- personally saw. With this column an auditor can prove, per seat, that the terms shown at
-- the moment of that signature are the terms that were paid.
--
-- Nullable because rows written before this migration genuinely do not know the answer, and
-- inventing one would be worse than admitting it. Every vote recorded from here on sets it.
ALTER TABLE consilium_voter
    ADD COLUMN payload_hash BYTEA CHECK (payload_hash IS NULL OR octet_length(payload_hash) = 32);

COMMENT ON COLUMN consilium_voter.payload_hash IS
    'The consilium payload hash this seat approved or rejected, captured at the moment of the vote. NULL only for votes cast before migration 0026.';

-- 2. THE SWEEPER'S QUERY.
--
-- `awaiting_execution` runs `WHERE state = 'approved' ORDER BY created_at` every 60 seconds
-- forever. Partial, because `approved` is a vanishingly small slice of the table — almost
-- every row is a terminal state — so this index stays a few pages no matter how much
-- governance history accumulates.
CREATE INDEX consilium_awaiting_execution_idx ON consilium (created_at) WHERE state = 'approved';

-- 3. WHEN THE OWNER ROSTER LAST MOVED.
--
-- WHY THIS TABLE EXISTS. The quorum rules compose weaker than any of them reads: at N=3 a
-- removal carries on ONE peer's vote, so two colluding owners can expel the third, admit
-- puppets by unanimity, and then reach a payout quorum entirely legitimately. No quorum
-- arrangement fixes that — a majority owns the roster. What can be bought is VISIBILITY: an
-- admission or removal freezes payout proposals long enough that the change cannot be
-- laundered into a payout in one uninterrupted motion, and the delay is itself the signal.
--
-- Only owner-affecting changes are recorded. A KYC change or an investor being promoted to
-- admin moves nobody in or out of the voting roster and must not delay a legitimate payout.
CREATE TABLE governance_roster_change (
    id         BIGSERIAL   PRIMARY KEY,
    user_id    UUID        NOT NULL REFERENCES users (id),
    from_role  TEXT        NOT NULL,
    to_role    TEXT        NOT NULL,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- One of the two ends must be `owner`, or the row does not belong here.
    CONSTRAINT governance_roster_change_touches_an_owner_seat CHECK (from_role = 'owner' OR to_role = 'owner'),
    CONSTRAINT governance_roster_change_is_a_change CHECK (from_role <> to_role)
);

-- The only read is "what is the most recent change?", so this is the whole access pattern.
CREATE INDEX governance_roster_change_recent_idx ON governance_roster_change (changed_at DESC);
