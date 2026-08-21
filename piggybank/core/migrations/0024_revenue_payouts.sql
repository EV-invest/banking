-- Revenue payouts — the fund paying its OWN earned money out to an operator wallet.
--
-- A payout runs the SAME saga as a user withdrawal (reserve -> dispatch -> settle/void),
-- through the same queue, watchers, dispatcher, reaper and reconciliation. The single
-- difference is which claim is debited: `fee` (retained fees + fee accruals — money the
-- fund earned) instead of `user:<uuid>`. So it lives in the `withdrawals` table rather
-- than a parallel one that would have to re-earn all of those guarantees.
--
-- `user_id` therefore becomes nullable, and `source` says which flow a row is. Every
-- existing row is a user withdrawal, which is exactly what the DEFAULT backfills.
ALTER TABLE withdrawals ALTER COLUMN user_id DROP NOT NULL;
ALTER TABLE withdrawals ADD COLUMN source TEXT NOT NULL DEFAULT 'user' CHECK (source IN ('user', 'revenue'));

-- The two columns are one fact, so the database — not just the mapper — refuses a row
-- that disagrees with itself. Without this a payout carrying a stray `user_id` would
-- surface in that user's wallet and withdrawal list as if it were their money.
ALTER TABLE withdrawals ADD CONSTRAINT withdrawals_source_matches_user
    CHECK ((source = 'user') = (user_id IS NOT NULL));

-- The payout history screen reads this slice; the existing `withdrawals_user_idx` only
-- covers the per-user reads (and skips NULL user_id rows entirely).
CREATE INDEX withdrawals_revenue_idx ON withdrawals (created_at DESC) WHERE source = 'revenue';

-- A user withdrawal's gross must exceed the network fee, so `amount ~ '^[1-9][0-9]*$'`
-- (from 0003) is already non-zero here. A payout charges NO fee (the fee claim is where
-- fees are retained — charging one would credit the money straight back), so `fee = '0'`
-- is expected on these rows and the existing `fee ~ '^[0-9]+$'` check already allows it.
