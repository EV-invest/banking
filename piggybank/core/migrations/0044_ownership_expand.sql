-- 0044: every unit of value has a holder — the EXPAND half (#245, phase 1).
--
-- The fund's own money has lived on two claim singletons with nobody behind them:
-- `fund` (seed capital, TB code 1) and `fee` (retained fees, code 40), plus the company's
-- in-kind stake `shares_company:<svc>` (code 63). None of them is anyone's. Phase 1 makes
-- the platform's money two ordinary allocations — `fee` and `fund` — that people hold
-- through units like any other product, and lets an allocation itself hold units of a
-- product (the fee class of `service_arb` is held by the `fee` allocation, and its
-- holders own it through their `fee` units). Hidden, never in the catalog, dealing only
-- with their holders.
--
-- (a) The two reserved registry rows. Fixed ids rather than minted — the domain refuses
-- to register a reserved slug (`Allocation::register`), so no operator path writes
-- these, and the code needs to know the ids to read the rows back:
--   uuid5(NAMESPACE_OID, 'evbanking:allocation:fee')  = 680240f5-1c41-58e2-987c-e52dbe2faa01
--   uuid5(NAMESPACE_OID, 'evbanking:allocation:fund') = 7533b905-9200-5ffb-b053-844c9c8da99a
-- (`domain::allocations::{FEE,FUND}_ALLOCATION_ID`; a hub test greps this file for the
-- same bytes). `open` because they deal — a holder redeems from them; `hidden` because
-- the catalog never shows them; `cash` because their claims hold USDT and a holder is
-- paid out of it. `ON CONFLICT DO NOTHING` so a re-run, or a database where an operator
-- once managed to register the slug, is left alone.
--
-- (b) `unit_issuances` learns the third holder kind, `allocation`, with its own column:
-- `holder_service` references the registry the way `holder_id` references `users`.
-- NOT reusing `holder_id` — it is a UUID with a foreign key to `users`, and the surrogate
-- id of an allocation is never its handle (its slug is). The pair CHECK mirrors the one
-- 0034 wrote for `user`: an `allocation` row names a service, every other kind does not.
-- The 0034 `holder_kind` CHECK was inline in the CREATE and auto-named
-- `unit_issuances_holder_kind_check`; it is recreated under the same name, widened.
--
-- EXPAND ONLY. `company` stays a legal `holder_kind` and `source`: the rows that hold
-- it are the record of the company stake that was minted and handed over, and the
-- retired `shares_company:<svc>` account is what the data migration (C-6) will debit.
-- Nothing here narrows a CHECK, drops a column or rewrites a row. The contract half
-- (dropping `company`, the `fund`/`fee` singleton parsing) is a later migration, after
-- the production balances have moved.
--
-- Backward compatible in both directions of a rolling deploy: a pod that predates this
-- migration never writes `holder_kind = 'allocation'` (it has no code path that does),
-- inserts without naming `holder_service` and lands on NULL, and reads issuance rows by
-- id or `(service, key)` — none of which it holds for an allocation row. The two
-- registry rows are `hidden`, so an old pod's catalog does not show them; its
-- `include_unlisted` admin listing does, wearing their titles, which is correct.
--
-- Migrations run at hub boot. Reversible while no `allocation` issuance row exists:
-- `ALTER TABLE unit_issuances DROP COLUMN holder_service` (drops its CHECK with it), the
-- `holder_kind` CHECK narrowed back to ('user', 'company'), and the two `allocations`
-- rows deleted — once an `allocation` row has been minted, its units are on the ledger
-- and the row must stay to explain them.

SET lock_timeout = '3s';
SET statement_timeout = '30s';

INSERT INTO allocations (id, service, title, summary, state, access, backing)
VALUES
    ('680240f5-1c41-58e2-987c-e52dbe2faa01', 'fee',  'Fee allocation',  'The platform''s earned fees, held by its people.', 'open', 'hidden', 'cash'),
    ('7533b905-9200-5ffb-b053-844c9c8da99a', 'fund', 'Fund allocation', 'The platform''s own capital, held by its people.', 'open', 'hidden', 'cash')
ON CONFLICT (service) DO NOTHING;

ALTER TABLE unit_issuances
    ADD COLUMN holder_service TEXT REFERENCES allocations (service),
    DROP CONSTRAINT unit_issuances_holder_kind_check,
    ADD CONSTRAINT unit_issuances_holder_kind_check CHECK (holder_kind IN ('user', 'company', 'allocation')),
    ADD CONSTRAINT unit_issuances_allocation_holder_names_a_service CHECK ((holder_kind = 'allocation') = (holder_service IS NOT NULL));

COMMENT ON COLUMN unit_issuances.holder_kind IS
    'user | company | allocation — who the units were minted to. `company` is the retired company stake (shares_company:<service>); `allocation` is a reserved allocation (holder_service) holding units of this product, e.g. `fee` holding a product''s fee class (shares_fee:<service>).';
COMMENT ON COLUMN unit_issuances.holder_service IS
    'The allocation holding the units when holder_kind = allocation (a reserved slug: fee | fund); NULL otherwise.';
