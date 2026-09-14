-- 0036: fee-policy changes — a ceiling on the rates, a versioned history with a notice
-- period, and the owners' consilium for a tightening beyond the house envelope (#233).
--
-- Until now `fee_policies` was a single upsert with no ceiling: one `AllocationManage`
-- holder could write 10000 bps and the next sweep collected it, with nobody told and no
-- record of what the terms had been. From here on a policy is never edited in place. A
-- CHANGE is recorded in `fee_policy_changes`, waits out the holders' notice (and, when it
-- tightens the terms beyond the house envelope, the owners' quorum), and is PROMOTED into
-- `fee_policies` by the fee sweeper once its moment arrives — after settling every
-- holder's accrual at the OLD rate (docs/FEES.md § "Changing the terms").
--
-- `fee_policies` keeps its meaning: the row in force RIGHT NOW, read by the sweeper and by
-- `carry_accrual`. It only learns which version it is and since when.

SET lock_timeout = '3s';

-- (1) The live row's version and the moment it took effect. Existing rows are version 1,
-- in force since they were last written — the only moment the old schema recorded.
ALTER TABLE fee_policies
    ADD COLUMN version        INTEGER     NOT NULL DEFAULT 1 CHECK (version >= 1),
    ADD COLUMN effective_from TIMESTAMPTZ NOT NULL DEFAULT now();
UPDATE fee_policies SET effective_from = updated_at;

-- (2) The history. Every row carries the FULL terms — a change is read on its own, never by
-- diffing against a neighbour — plus the requirement it was held to, who asked and when, the
-- consilium it waited on (if any), and the two moments that matter: when it was scheduled
-- (the notice clock starts here) and when it was applied.
--
-- `version` is unique per product and counts every request, carried or not: a rejected
-- change still happened, and the number on the live row says which request produced it.
CREATE TABLE fee_policy_changes (
    id              UUID        PRIMARY KEY,
    service         TEXT        NOT NULL REFERENCES allocations (service) ON DELETE CASCADE,
    version         INTEGER     NOT NULL CHECK (version >= 1),
    management_bps  INTEGER     NOT NULL CHECK (management_bps BETWEEN 0 AND 10000),
    performance_bps INTEGER     NOT NULL CHECK (performance_bps BETWEEN 0 AND 10000),
    hurdle_bps      INTEGER     NOT NULL CHECK (hurdle_bps BETWEEN 0 AND 10000),
    basis           TEXT        NOT NULL CHECK (basis IN ('invested_capital', 'market_value')),
    crystallization TEXT        NOT NULL CHECK (crystallization IN ('monthly', 'quarterly', 'semi_annual', 'annual')),
    -- awaiting_consilium → scheduled → active → superseded; awaiting_consilium → rejected
    -- (the consilium refused, expired, was withdrawn or voided); scheduled → cancelled (an
    -- administrator withdrew it before it took effect).
    state           TEXT        NOT NULL CHECK (state IN ('awaiting_consilium', 'scheduled', 'active', 'superseded', 'rejected', 'cancelled')),
    requirement     TEXT        NOT NULL CHECK (requirement IN ('admin', 'owner_consilium')),
    -- When the terms bind. Provisional while awaiting the owners (the notice clock has not
    -- started); fixed at scheduling, never earlier than 24h after it while anyone holds units.
    effective_from  TIMESTAMPTZ NOT NULL,
    requested_by    TEXT        NOT NULL,
    requested_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Why, in the requester's words. Shown to the owners and the holders, never interpreted.
    reason          TEXT        NOT NULL DEFAULT '',
    consilium_id    UUID        REFERENCES consilium (id),
    scheduled_at    TIMESTAMPTZ,
    applied_at      TIMESTAMPTZ,
    closed_reason   TEXT,

    UNIQUE (service, version),
    -- A change needing the owners always names its consilium, and one that does not never
    -- does — the two are written in one transaction, so there is no in-between row.
    CONSTRAINT fee_policy_change_consilium_matches_requirement CHECK ((requirement = 'owner_consilium') = (consilium_id IS NOT NULL)),
    -- The owners are never asked to approve a change with no stated reason: the approval
    -- mail is refused without one.
    CONSTRAINT fee_policy_change_owners_are_told_why CHECK (requirement <> 'owner_consilium' OR length(reason) > 0),
    -- The notice clock is stamped exactly once, when the change becomes scheduled, and stays
    -- on the row through cancellation so the history says when the notice was given.
    CONSTRAINT fee_policy_change_scheduled_is_stamped CHECK (state NOT IN ('scheduled', 'active', 'superseded') OR scheduled_at IS NOT NULL),
    CONSTRAINT fee_policy_change_awaiting_is_unstamped CHECK (state <> 'awaiting_consilium' OR (scheduled_at IS NULL AND applied_at IS NULL)),
    CONSTRAINT fee_policy_change_applied_is_stamped CHECK ((state IN ('active', 'superseded')) = (applied_at IS NOT NULL)),
    -- A closed change always says why; nothing else carries a reason.
    CONSTRAINT fee_policy_change_closed_states_why CHECK ((state IN ('rejected', 'cancelled')) = (closed_reason IS NOT NULL))
);

-- AT MOST ONE CHANGE ON ITS WAY PER PRODUCT. Two pending changes would leave the holders
-- notified of terms that never arrive, so a second request is refused until the first is
-- cancelled — the database's statement, not the write path's habit.
CREATE UNIQUE INDEX fee_policy_changes_single_pending_idx ON fee_policy_changes (service) WHERE state IN ('awaiting_consilium', 'scheduled');
-- Exactly one active change per product mirrors the one live row in `fee_policies`.
CREATE UNIQUE INDEX fee_policy_changes_single_active_idx ON fee_policy_changes (service) WHERE state = 'active';
-- The sweeper's probe: scheduled changes whose moment has come.
CREATE INDEX fee_policy_changes_due_idx ON fee_policy_changes (effective_from) WHERE state = 'scheduled';
CREATE INDEX fee_policy_changes_consilium_idx ON fee_policy_changes (consilium_id) WHERE consilium_id IS NOT NULL;
CREATE INDEX fee_policy_changes_history_idx ON fee_policy_changes (service, version DESC);

-- (3) Backfill: every live policy becomes version 1 of its own history, `active`, with the
-- one timestamp the old schema kept standing in for all of them. Done BEFORE the ceilings
-- below are added, so a pre-existing row above them is carried into the history as it is
-- rather than aborting the boot.
INSERT INTO fee_policy_changes (id, service, version, management_bps, performance_bps, hurdle_bps, basis, crystallization,
                                state, requirement, effective_from, requested_by, requested_at, scheduled_at, applied_at)
SELECT gen_random_uuid(), service, 1, management_bps, performance_bps, hurdle_bps, basis, crystallization,
       'active', 'admin', updated_at, updated_by, updated_at, updated_at, updated_at
FROM fee_policies;

-- (4) THE CEILINGS: 5% p.a. management, 50% performance — `MAX_MANAGEMENT_BPS` and
-- `MAX_PERFORMANCE_BPS` in domain/src/fees.rs, stated here too so no write path, not even a
-- direct one, can put a higher figure where the sweeper reads it. `NOT VALID`: a row that
-- already exceeds them must not stop the pod from booting; every INSERT and UPDATE from
-- here on is held to them.
--
-- On the history table the ceiling holds only while a row can still bind — pending or
-- active. A `NOT VALID` CHECK is re-evaluated on UPDATE, and the one UPDATE a legacy row
-- above the ceiling ever receives is `SET state = 'superseded'` when its replacement is
-- promoted: an unconditional ceiling would refuse exactly that, roll back the promotion
-- every minute, and make an over-the-ceiling policy the one policy that can never be
-- lowered — the very case #233 exists to end. A closed row is history, and history is not
-- held to today's ceiling.
ALTER TABLE fee_policies ADD CONSTRAINT fee_policies_management_ceiling CHECK (management_bps <= 500) NOT VALID;
ALTER TABLE fee_policies ADD CONSTRAINT fee_policies_performance_ceiling CHECK (performance_bps <= 5000) NOT VALID;
ALTER TABLE fee_policy_changes ADD CONSTRAINT fee_policy_changes_management_ceiling
    CHECK (state IN ('superseded', 'rejected', 'cancelled') OR management_bps <= 500) NOT VALID;
ALTER TABLE fee_policy_changes ADD CONSTRAINT fee_policy_changes_performance_ceiling
    CHECK (state IN ('superseded', 'rejected', 'cancelled') OR performance_bps <= 5000) NOT VALID;

-- (5) The fourth consilium kind, after 0035's `valuation_override`. ONE COMMIT WITH THE RUST
-- ARM, for the reason 0029 and 0031 state: `kind` decides how `terms` is read, and a row of
-- a kind the binary cannot parse fails every read of the governance history.
ALTER TABLE consilium DROP CONSTRAINT consilium_kind_check;
ALTER TABLE consilium ADD CONSTRAINT consilium_kind_check CHECK (kind IN ('revenue_payout', 'payment', 'valuation_override', 'fee_policy'));

-- Its effect: the change it scheduled. RESTRICT (the default) on delete, as the payment
-- link is — a consilium must not lose the record of what it carried.
ALTER TABLE consilium ADD COLUMN executed_fee_policy_change_id UUID REFERENCES fee_policy_changes (id);

-- EXACTLY ONE EFFECT ON AN EXECUTED ROW, over the FULL list of effect columns — the two
-- 0029/0031 knew, 0035's `executed_valuation_id`, and this one. Recomputed here in one
-- DROP + ADD rather than patched, so any sibling migration adding an effect column of its
-- own conflicts here visibly and the list is reconciled by hand.
ALTER TABLE consilium DROP CONSTRAINT consilium_execution_is_recorded;
ALTER TABLE consilium ADD CONSTRAINT consilium_execution_is_recorded
    CHECK ((state = 'executed') = (num_nonnulls(executed_withdrawal_id, executed_payment_id, executed_valuation_id, executed_fee_policy_change_id) = 1));

-- `consilium_payout_spends_the_fee_claim` (0029) is left alone: it constrains only
-- `revenue_payout`, and a fee-policy row carries the product's `shares_fee:<service>` —
-- which is what makes `consilium_single_open_per_source_idx` yield one open fee-policy
-- consilium PER PRODUCT without blocking a payout or a payment over another claim.

-- (6) The governance mail queue learns the two fee-policy kinds and the third subject a row
-- can be about: the change whose holders are being notified. The approval mails to the
-- owners name the consilium, as every approval does.
ALTER TABLE consilium_mail ADD COLUMN fee_policy_change_id UUID REFERENCES fee_policy_changes (id) ON DELETE CASCADE;
ALTER TABLE consilium_mail DROP CONSTRAINT consilium_mail_names_one_subject;
ALTER TABLE consilium_mail ADD CONSTRAINT consilium_mail_names_one_subject CHECK (num_nonnulls(consilium_id, payment_id, fee_policy_change_id) = 1);
ALTER TABLE consilium_mail DROP CONSTRAINT consilium_mail_kind_check;
ALTER TABLE consilium_mail ADD CONSTRAINT consilium_mail_kind_check
    CHECK (kind IN ('payout_approval', 'payout_outcome', 'token_burned', 'payment_consent', 'payment_approval', 'fee_policy_approval', 'fee_policy_notice'));
CREATE INDEX consilium_mail_fee_policy_change_idx ON consilium_mail (fee_policy_change_id) WHERE fee_policy_change_id IS NOT NULL;
