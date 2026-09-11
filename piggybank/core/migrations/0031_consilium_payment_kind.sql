-- The second consilium kind: a payment order whose source is fund-owned money.
--
-- WHY THIS IS ONE COMMIT WITH THE RUST ARM, not a schema change landed ahead of it.
-- `0029_consilium_source_claim.sql` states the rule and the reason: `kind` decides how the
-- `terms` JSONB is read, so a row of a kind the binary cannot parse does not merely fail its
-- own read — `ConsiliumKind::parse` refuses it and every read of the governance HISTORY
-- (the owners' list screen, the execution sweep, the roster-change void) fails with it. The
-- database's vocabulary and the domain's therefore move together, in both directions:
-- widening the CHECK before `ConsiliumKind::Payment` exists would admit an unreadable row,
-- and shipping the variant before the CHECK would refuse every write of it.
--
-- The constraint is the inline one Postgres named `consilium_kind_check`, so widening it is
-- DROP + ADD. No row can be of the new kind yet, so the ADD validates instantly.
ALTER TABLE consilium DROP CONSTRAINT consilium_kind_check;
ALTER TABLE consilium ADD CONSTRAINT consilium_kind_check CHECK (kind IN ('revenue_payout', 'payment'));

-- `consilium_payout_spends_the_fee_claim` (0029) is deliberately left alone. It reads
-- `kind <> 'revenue_payout' OR source_claim = 'fee'`, so a payment row is already exempt and
-- carries whatever claim its order debits — which is the whole point of naming the claim.
-- `consilium_single_open_per_source_idx` then serializes a payment consilium against a
-- revenue payout only when the two actually spend the same claim.
