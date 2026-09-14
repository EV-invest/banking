-- The third consilium kind: a NAV mark the move guard refuses (banking#232).
--
-- WHY THIS IS ONE COMMIT WITH THE RUST ARM, not a schema change landed ahead of it.
-- `0029_consilium_source_claim.sql` states the rule and `0031_consilium_payment_kind.sql`
-- applied it: `kind` decides how the `terms` JSONB is read, so a row of a kind the binary
-- cannot parse does not merely fail its own read — `ConsiliumKind::parse` refuses it and
-- every read of the governance HISTORY (the owners' list screen, the execution sweep, the
-- roster-change void) fails with it. The database's vocabulary and the domain's therefore
-- move together, in both directions: widening the CHECK before
-- `ConsiliumKind::ValuationOverride` exists would admit an unreadable row, and shipping the
-- variant before the CHECK would refuse every write of it.
--
-- WHY A CONSILIUM AT ALL. `PostFundValuation` used to carry an `override` flag that let the
-- poster lift their own NAV-move guard, so one admin could inflate NAV, redeem at the
-- inflated price out of the fund's pooled claim and withdraw. The flag is gone; a move past
-- the guard is now put to the owners, and only an executed consilium records it.
--
-- The kind constraint is the inline one Postgres named `consilium_kind_check`, so widening
-- it is DROP + ADD. No row can be of the new kind yet, so the ADD validates instantly.
-- `consilium` is a governance table of tens of rows, so every statement here takes a lock
-- for no measurable time; the timeout is the ordinary precaution against queueing behind
-- something else.

SET lock_timeout = '3s';

ALTER TABLE consilium DROP CONSTRAINT consilium_kind_check;
ALTER TABLE consilium ADD CONSTRAINT consilium_kind_check CHECK (kind IN ('revenue_payout', 'payment', 'valuation_override'));

-- `consilium_payout_spends_the_fee_claim` (0029) is deliberately left alone: a valuation
-- override row is exempt from it (`kind <> 'revenue_payout'`) and carries the claim of the
-- fund it marks (`service:<id>`), so `consilium_single_open_per_source_idx` serializes it
-- against a payment out of that same claim and against nothing else. One open override per
-- fund is that index's statement, not the write path's habit.

-- The third effect an approved consilium can produce: the `fund_valuations` mark it
-- recorded. Nullable, no default, no `ON DELETE` — marks are append-only and never deleted.
ALTER TABLE consilium ADD COLUMN executed_valuation_id UUID REFERENCES fund_valuations (id);

-- EXACTLY ONE EFFECT ON AN EXECUTED ROW, NONE ON ANY OTHER — now over three columns. Every
-- existing executed row has exactly one of the first two set, so the widened CHECK
-- validates instantly.
ALTER TABLE consilium DROP CONSTRAINT consilium_execution_is_recorded;
ALTER TABLE consilium ADD CONSTRAINT consilium_execution_is_recorded
    CHECK ((state = 'executed') = (num_nonnulls(executed_withdrawal_id, executed_payment_id, executed_valuation_id) = 1));

COMMENT ON COLUMN consilium.executed_valuation_id IS
    'The fund_valuations row an executed valuation_override consilium recorded. NULL for every other kind and every other state.';
