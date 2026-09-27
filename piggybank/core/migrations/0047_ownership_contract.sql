-- 0047: every unit of value has a holder — the CONTRACT half (#245, phase 1, C-9).
--
-- 0045/0046 widened the schema for the reserved `fee` / `fund` allocations and left every
-- retired value legal, so the one-off data command could move production across. It ran
-- on 2026-09-27 (v1.0.0): the retired `fund` claim was handed to `service:fund`, the
-- retired `fee` claim was already empty, the reconciliation came back clean. This
-- migration narrows the CHECKs to the vocabulary the binary still speaks: a `Party` is
-- `user | service`, a withdrawal is a user's, a consilium is never a revenue payout, a
-- unit holder is a person or a reserved allocation, and a deposit lands on a person.
--
-- WHAT PRODUCTION HOLDS, read-only, on the day this was written:
--   payments                        0 rows
--   withdrawals WHERE source='revenue'   0 rows   (the retired payouts lived here — 0024
--                                                   added the column; there never was a
--                                                   `revenue_payouts` table)
--   consilium                       0 rows of any kind
--   consilium_mail                  0 rows
--   deposits                        3 rows party_kind='piggybank' (the seed history), 2 'user'
--   unit_issuances                  1 row holder_kind='company', source='mint' (the 13 000
--                                   service_arb units minted to the company stake) and
--                                   1 row holder_kind='user', source='company' (their
--                                   hand-over to a person); the rest user/allocation
--                                   mint/retire
--
-- So the first four tables are narrowed by ordinary CHECKs, validated instantly over no
-- offending row. The last two keep history the new vocabulary cannot name, and history is
-- not rewritten: their CHECKs are added `NOT VALID`. Postgres then enforces them on every
-- INSERT and on every UPDATE of a row, and never re-reads the rows already there. No code
-- path updates those rows — the only UPDATE of `deposits` (the sweeper's `swept_at`) is
-- scoped to `party_kind = 'user'`, and the only UPDATE of `unit_issuances` (the relay's
-- `queued` -> `applied` stamp) matches `state = 'queued'`, which a historical row is not.
-- Do NOT `VALIDATE CONSTRAINT` these two: it would fail on the history by design.
--
-- The old wide CHECKs are dropped rather than kept beside the narrow ones: the rows they
-- still describe are frozen, and two CHECKs over one column that disagree read as a bug.
-- `unit_issuances_company_source_names_a_user` (0038) goes too — it constrained a value
-- nothing may write any more; the two historical rows satisfied it when written.
--
-- `consilium_mail.kind` loses `payout_approval`: it was the revenue-payout invitation and
-- nothing else rides it (a holder grant's and a seed's ride `payment_approval`, per 0046).
-- `payout_outcome` stays — every kind's verdict mail is that template.
--
-- `tb_accounts` is left alone. The retired rows `fund` (code 1), `fee` (40) and
-- `shares_company:<svc>` (63) name TigerBeetle accounts, which cannot be deleted; they
-- hold zero, the binary classifies them by code and never resolves them.
--
-- Backward compatible with the release before it (v1.0.0) for everything it can still
-- do: that binary refuses every retired value at the RPC boundary already, so the only
-- write it could attempt that this refuses is the replay of a pre-#245 outbox row — and
-- production has none pending (outbox pending 0, parked 0).
--
-- Migrations run at hub boot. Reversible by a later migration while no row relies on the
-- narrow vocabulary (none can: it is a subset). Rollback, statement for statement:
--   ALTER TABLE payments DROP CONSTRAINT payments_from_kind_check, DROP CONSTRAINT payments_to_kind_check,
--     ADD CONSTRAINT payments_from_kind_check CHECK (from_kind IN ('piggybank', 'user', 'service', 'revenue')),
--     ADD CONSTRAINT payments_to_kind_check CHECK (to_kind IN ('piggybank', 'user', 'service', 'revenue'));
--   ALTER TABLE withdrawals DROP CONSTRAINT withdrawals_source_check,
--     ADD CONSTRAINT withdrawals_source_check CHECK (source IN ('user', 'revenue'));
--   ALTER TABLE consilium DROP CONSTRAINT consilium_kind_check,
--     ADD CONSTRAINT consilium_kind_check CHECK (kind IN ('revenue_payout', 'payment', 'valuation_override', 'fee_policy', 'holder_grant', 'seed_capital')),
--     ADD CONSTRAINT consilium_payout_spends_the_fee_claim CHECK (kind <> 'revenue_payout' OR source_claim = 'fee');
--   ALTER TABLE consilium_mail DROP CONSTRAINT consilium_mail_kind_check,
--     ADD CONSTRAINT consilium_mail_kind_check CHECK (kind IN ('payout_approval', 'payout_outcome', 'token_burned', 'payment_consent', 'payment_approval', 'fee_policy_approval', 'fee_policy_notice'));
--   ALTER TABLE deposits DROP CONSTRAINT deposits_party_is_a_user,
--     ADD CONSTRAINT deposits_party_is_never_revenue CHECK (party_kind <> 'revenue');
--   ALTER TABLE unit_issuances DROP CONSTRAINT unit_issuances_holder_kind_check, DROP CONSTRAINT unit_issuances_source_check,
--     ADD CONSTRAINT unit_issuances_holder_kind_check CHECK (holder_kind IN ('user', 'company', 'allocation')),
--     ADD CONSTRAINT unit_issuances_source_check CHECK (source IN ('mint', 'company', 'retire')),
--     ADD CONSTRAINT unit_issuances_company_source_names_a_user CHECK (source <> 'company' OR holder_kind = 'user');
-- A rolled-back binary still cannot write a retired value (v1.0.0 refuses them itself);
-- the rollback only matters to a binary older than that.

SET lock_timeout = '3s';
SET statement_timeout = '30s';

ALTER TABLE payments
    DROP CONSTRAINT payments_from_kind_check,
    DROP CONSTRAINT payments_to_kind_check,
    ADD CONSTRAINT payments_from_kind_check CHECK (from_kind IN ('user', 'service')),
    ADD CONSTRAINT payments_to_kind_check CHECK (to_kind IN ('user', 'service'));

ALTER TABLE withdrawals
    DROP CONSTRAINT withdrawals_source_check,
    ADD CONSTRAINT withdrawals_source_check CHECK (source IN ('user'));

ALTER TABLE consilium
    DROP CONSTRAINT consilium_payout_spends_the_fee_claim,
    DROP CONSTRAINT consilium_kind_check,
    ADD CONSTRAINT consilium_kind_check CHECK (kind IN ('payment', 'valuation_override', 'fee_policy', 'holder_grant', 'seed_capital'));

ALTER TABLE consilium_mail
    DROP CONSTRAINT consilium_mail_kind_check,
    ADD CONSTRAINT consilium_mail_kind_check CHECK (kind IN ('payout_outcome', 'token_burned', 'payment_consent', 'payment_approval', 'fee_policy_approval', 'fee_policy_notice'));

-- NOT VALID: the three `piggybank` rows are the seed's history and stay as written.
ALTER TABLE deposits
    DROP CONSTRAINT deposits_party_is_never_revenue,
    ADD CONSTRAINT deposits_party_is_a_user CHECK (party_kind = 'user') NOT VALID;

-- NOT VALID: the company stake's mint and its hand-over stay as written.
ALTER TABLE unit_issuances
    DROP CONSTRAINT unit_issuances_company_source_names_a_user,
    DROP CONSTRAINT unit_issuances_holder_kind_check,
    DROP CONSTRAINT unit_issuances_source_check,
    ADD CONSTRAINT unit_issuances_holder_kind_check CHECK (holder_kind IN ('user', 'allocation')) NOT VALID,
    ADD CONSTRAINT unit_issuances_source_check CHECK (source IN ('mint', 'retire')) NOT VALID;

COMMENT ON COLUMN unit_issuances.holder_kind IS
    'user | allocation — who the units were minted to (allocation: a reserved allocation named by holder_service holding this product''s fee class). Rows written before #245 may read `company` (the retired company stake); the CHECK is NOT VALID so they stay, and nothing writes it.';
COMMENT ON COLUMN unit_issuances.source IS
    'mint | retire — minted in kind (Dr holder shares / Cr shares_outstanding) or retired (Dr shares_outstanding / Cr holder shares). Rows written before #245 may read `company` (a hand-over out of the retired company stake, supply unchanged); the CHECK is NOT VALID so they stay. units is always the magnitude.';
COMMENT ON COLUMN deposits.party_kind IS
    'Always `user` for a new row. Rows written before #245 may read `piggybank` (the seed deposits onto the retired fund claim); the CHECK is NOT VALID so they stay as history.';
