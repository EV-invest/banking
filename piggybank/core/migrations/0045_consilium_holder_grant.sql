-- 0045: the fifth consilium kind — a holder grant: units of a reserved allocation (`fee`,
-- `fund`) minted to a person on the owners' quorum (#245, phase 1).
--
-- WHY THIS IS ONE COMMIT WITH THE RUST ARM, not a schema change landed ahead of it.
-- `0029_consilium_source_claim.sql` states the rule and 0031/0035/0036 applied it: `kind`
-- decides how the `terms` JSONB is read, so a row of a kind the binary cannot parse does
-- not merely fail its own read — `ConsiliumKind::parse` refuses it and every read of the
-- governance HISTORY (the owners' list screen, the execution sweep, the roster-change void)
-- fails with it. The database's vocabulary and the domain's therefore move together, in
-- both directions: widening the CHECK before `ConsiliumKind::HolderGrant` exists would
-- admit an unreadable row, and shipping the variant before the CHECK would refuse every
-- write of it.
--
-- WHY A CONSILIUM AT ALL. The `fee` and `fund` allocations are the platform's own money,
-- held by people through units (0044). Minting units of them is how a holder is seated —
-- and it dilutes every holder already there. `IssueUnits` used to let one
-- `AllocationManage` holder do that on their own say-so; from here on the direct RPC
-- refuses a reserved allocation and the mint happens only as the effect of an executed
-- holder-grant consilium (or the one-off data migration that seeds the first holders).
--
-- The kind constraint is the inline one Postgres named `consilium_kind_check`, so widening
-- it is DROP + ADD. No row can be of the new kind yet, so the ADD validates instantly.
-- `consilium` is a governance table of tens of rows, so every statement here takes a lock
-- for no measurable time; the timeout is the ordinary precaution against queueing behind
-- something else.
--
-- EXPAND ONLY. Nothing is narrowed, dropped or rewritten. Reversible while no `holder_grant`
-- row exists: drop the column (its CHECK goes with it), recompute
-- `consilium_execution_is_recorded` over the four 0036 columns, narrow the kind CHECK back.

SET lock_timeout = '3s';
SET statement_timeout = '30s';

ALTER TABLE consilium DROP CONSTRAINT consilium_kind_check;
ALTER TABLE consilium ADD CONSTRAINT consilium_kind_check
    CHECK (kind IN ('revenue_payout', 'payment', 'valuation_override', 'fee_policy', 'holder_grant'));

-- `consilium_payout_spends_the_fee_claim` (0029) is left alone: it constrains only
-- `revenue_payout`, and a holder-grant row carries the allocation's own claim
-- (`service:fee` / `service:fund`) — which is what makes `consilium_single_open_per_source_idx`
-- yield one open grant PER RESERVED ALLOCATION, serialized against a payment out of that
-- same claim and against nothing else.

-- Its effect: the in-kind issuance it minted. RESTRICT (the default) on delete, as the
-- payment and fee-policy links are — a consilium must not lose the record of what it
-- carried.
ALTER TABLE consilium ADD COLUMN executed_issuance_id UUID REFERENCES unit_issuances (id);

-- EXACTLY ONE EFFECT ON AN EXECUTED ROW, over the FULL list of effect columns — the four
-- 0036 knew and this one. Recomputed in one DROP + ADD rather than patched, so any sibling
-- migration adding an effect column of its own conflicts here visibly and the list is
-- reconciled by hand. Every existing executed row has exactly one of the first four set,
-- so the widened CHECK validates instantly.
ALTER TABLE consilium DROP CONSTRAINT consilium_execution_is_recorded;
ALTER TABLE consilium ADD CONSTRAINT consilium_execution_is_recorded
    CHECK ((state = 'executed') = (num_nonnulls(executed_withdrawal_id, executed_payment_id, executed_valuation_id, executed_fee_policy_change_id, executed_issuance_id) = 1));

COMMENT ON COLUMN consilium.executed_issuance_id IS
    'The unit_issuances row an executed holder_grant consilium minted. NULL for every other kind and every other state.';

-- `consilium_mail.kind` IS DELIBERATELY NOT WIDENED. A holder grant's invitation rides the
-- `payment_approval` template and its verdicts the `payout_outcome` one — the same
-- borrowing 0035's valuation override made (EV-invest/concierge#94 tracks the dedicated
-- kinds) — so no new mail kind exists for the CHECK to admit.
