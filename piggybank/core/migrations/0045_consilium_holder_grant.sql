-- 0045: the fifth and sixth consilium kinds — a holder grant: units of a reserved
-- allocation (`fee`, `fund`) minted to a person on the owners' quorum; and a seed of
-- capital: a chain-proven arrival on the treasury attributed to a person as their deposit
-- and their subscription into `fund`, on the same quorum (#245, phase 1).
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
-- A SEED IS THE SAME DECISION BY ANOTHER ROUTE. The chain proves that a dollar reached the
-- treasury, never whose it was; `SeedCapital` used to let one `CapitalManage` holder book
-- it as their own deposit and take `fund` units for it — the first administrator to name
-- a reference won it. From here on the RPC opens a `seed_capital` consilium and the units
-- are seated only when the owners' quorum executes it.
--
-- The kind constraint is the inline one Postgres named `consilium_kind_check`, so widening
-- it is DROP + ADD. No row can be of the new kind yet, so the ADD validates instantly.
-- `consilium` is a governance table of tens of rows, so every statement here takes a lock
-- for no measurable time; the timeout is the ordinary precaution against queueing behind
-- something else.
--
-- EXPAND ONLY. Nothing is narrowed, dropped or rewritten. Reversible while no `holder_grant`
-- or `seed_capital` row exists: drop the two columns (their CHECK goes with them), recompute
-- `consilium_execution_is_recorded` over the four 0036 columns, narrow the kind CHECK back.

SET lock_timeout = '3s';
SET statement_timeout = '30s';

ALTER TABLE consilium DROP CONSTRAINT consilium_kind_check;
ALTER TABLE consilium ADD CONSTRAINT consilium_kind_check
    CHECK (kind IN ('revenue_payout', 'payment', 'valuation_override', 'fee_policy', 'holder_grant', 'seed_capital'));

-- `consilium_payout_spends_the_fee_claim` (0029) is left alone: it constrains only
-- `revenue_payout`, and a holder-grant row carries the allocation's own claim
-- (`service:fee` / `service:fund`) — which is what makes `consilium_single_open_per_source_idx`
-- yield one open grant PER RESERVED ALLOCATION, serialized against a payment out of that
-- same claim and against nothing else. A seed row carries `service:fund`, so it takes the
-- fund allocation's slot: one open seed, never beside an open grant of `fund` units.

-- Its effect: the in-kind issuance it minted. RESTRICT (the default) on delete, as the
-- payment and fee-policy links are — a consilium must not lose the record of what it
-- carried.
ALTER TABLE consilium ADD COLUMN executed_issuance_id UUID REFERENCES unit_issuances (id);

-- A seed's effect: the `fund` subscription it opened for the depositor. The deposit itself
-- is keyed by the chain reference and has no id to link; the subscription's id is derived
-- from that reference (`balance::seed_subscription_id`) and is the fact that seats the
-- holder. RESTRICT on delete, as the other effect links are.
ALTER TABLE consilium ADD COLUMN executed_subscription_id UUID REFERENCES subscriptions (id);

-- EXACTLY ONE EFFECT ON AN EXECUTED ROW, over the FULL list of effect columns — the four
-- 0036 knew and the two here. Recomputed in one DROP + ADD rather than patched, so any
-- sibling migration adding an effect column of its own conflicts here visibly and the list
-- is reconciled by hand. Every existing executed row has exactly one of the first four
-- set, so the widened CHECK validates instantly.
ALTER TABLE consilium DROP CONSTRAINT consilium_execution_is_recorded;
ALTER TABLE consilium ADD CONSTRAINT consilium_execution_is_recorded
    CHECK ((state = 'executed') = (num_nonnulls(executed_withdrawal_id, executed_payment_id, executed_valuation_id, executed_fee_policy_change_id, executed_issuance_id, executed_subscription_id) = 1));

COMMENT ON COLUMN consilium.executed_issuance_id IS
    'The unit_issuances row an executed holder_grant consilium minted. NULL for every other kind and every other state.';
COMMENT ON COLUMN consilium.executed_subscription_id IS
    'The subscriptions row an executed seed_capital consilium opened for the depositor. NULL for every other kind and every other state.';

-- `consilium_mail.kind` IS DELIBERATELY NOT WIDENED. A holder grant's and a seed's
-- invitations ride the `payment_approval` template and their verdicts the `payout_outcome`
-- one — the same borrowing 0035's valuation override made (EV-invest/concierge#94 tracks
-- the dedicated kinds) — so no new mail kind exists for the CHECK to admit.
