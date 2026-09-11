-- Consilium, generalized: name the claim a request spends, and make "one open request" a
-- statement about that claim rather than about the whole table.
--
-- NOTHING IN THIS MIGRATION CHANGES BEHAVIOUR TODAY. There is still exactly one kind
-- (`revenue_payout`), it still spends `fee`, and the new index is therefore bit-equivalent
-- to the old one over every row that exists or that this release can write. It is the
-- preparation a second kind needs, landed on its own so the change that does alter
-- behaviour arrives without schema work attached to it.
--
-- WHY `kind` IS NOT WIDENED HERE. Adding a value to the CHECK would let the database hold a
-- row this release cannot produce and, worse, cannot LOAD: `ConsiliumKind::parse` refuses
-- anything but `revenue_payout`, so such a row would fail every read of the governance
-- history, not just its own. The CHECK is widened by the migration that introduces the
-- variant, in the same commit as the Rust arm — the database's vocabulary and the domain's
-- stay the same size. (The constraint is the inline one Postgres named `consilium_kind_check`;
-- widening it is DROP CONSTRAINT + ADD CONSTRAINT.)
--
-- BACK-COMPATIBILITY, STATED PLAINLY. `source_claim` is NOT NULL with no default, so a
-- binary from before this release that opened a consilium against this schema would be
-- refused by the NOT NULL. That is a loud, retryable failure confined to one rare governance
-- RPC during the rollout window, and it is preferred over a lingering `DEFAULT 'fee'` that
-- would silently file a future payment against the fund's revenue claim. Every other path —
-- voting, execution, sweeps, the history screen — is untouched.

-- The claim the request spends from, as `LedgerAccountKey::logical_key()` writes it
-- (`fee`, `user:<uuid>`, `fund`, `service:<id>`). Added nullable, backfilled, then tightened,
-- so the rewrite-free order holds even once this table is not small.
ALTER TABLE consilium ADD COLUMN source_claim TEXT;

-- TOTAL BY CONSTRUCTION. `kind` admits one value, so this touches every row, and a revenue
-- payout debits `fee` and nothing else — which is what makes the index swap below a rename
-- rather than a change of rule.
UPDATE consilium SET source_claim = 'fee' WHERE kind = 'revenue_payout';

ALTER TABLE consilium ALTER COLUMN source_claim SET NOT NULL;
ALTER TABLE consilium ADD CONSTRAINT consilium_source_claim_is_named CHECK (length(source_claim) > 0);
-- The one fact that makes the new index equivalent to the old one, stated to the database so
-- it stays true: a revenue payout spends the fund's revenue claim, always.
ALTER TABLE consilium ADD CONSTRAINT consilium_payout_spends_the_fee_claim CHECK (kind <> 'revenue_payout' OR source_claim = 'fee');

-- AT MOST ONE OPEN REQUEST **PER SOURCE CLAIM**, replacing "at most one in the database".
--
-- The old `consilium_single_open_idx ON consilium ((TRUE)) WHERE state = 'open'` indexed
-- every open row under one constant key. Over the rows that exist, `source_claim` is that
-- same constant (`'fee'`, by the backfill and the CHECK above), so the new index indexes
-- exactly the same rows under exactly the same single key: the invariant "one open payout at
-- a time" is preserved word for word, and the existing consilium suite passes unchanged.
-- What it stops doing is blocking a request over a DIFFERENT claim, which is the whole point
-- of naming the claim.
--
-- Both statements run inside the migration's transaction: `consilium` is a governance table
-- of tens of rows, so building the index takes no measurable lock, and CONCURRENTLY (which
-- cannot run in a transaction) would cost the atomicity of the swap for nothing.
CREATE UNIQUE INDEX consilium_single_open_per_source_idx ON consilium (source_claim) WHERE state = 'open';
DROP INDEX consilium_single_open_idx;

-- The second effect an approved consilium will be able to produce. Always NULL until the
-- kind that writes it exists — `rehydrate` has no variant to map it onto yet, which is safe
-- precisely because nothing can set it. The CHECK below is what will keep it honest.
ALTER TABLE consilium ADD COLUMN executed_payment_id UUID;

-- EXACTLY ONE EFFECT ON AN EXECUTED ROW, NONE ON ANY OTHER. Replaces the single-column form
-- (`(state = 'executed') = (executed_withdrawal_id IS NOT NULL)`), which would have read a
-- payment-effect row as a consilium that executed nothing. `num_nonnulls(...) = 1` also
-- forbids a row claiming both.
ALTER TABLE consilium DROP CONSTRAINT consilium_execution_is_recorded;
ALTER TABLE consilium ADD CONSTRAINT consilium_execution_is_recorded
    CHECK ((state = 'executed') = (num_nonnulls(executed_withdrawal_id, executed_payment_id) = 1));
