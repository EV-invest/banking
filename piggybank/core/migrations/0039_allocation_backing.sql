-- 0039: what stands behind a product's units, and the third issuance source — retire.
--
-- (a) `allocations.backing`. Every unit a subscription mints has cash behind it: the
-- investor's claim moved into the fund's (`service:<svc>`), and a redemption prices the
-- units at NAV and pays that cash back out. Units minted in kind (`unit_issuances`, 0034)
-- have none — the product was registered against an asset the holders already own, and
-- the fund's USDT claim never saw a cent for them. A redemption on such a product asks
-- the fund to pay cash it does not hold: it queues forever, or — once the fund holds
-- cash for some OTHER reason — pays out money that belongs to someone else. This column
-- names the distinction so the redeem path can refuse (`in_kind`) or admit (`cash`) it
-- before any money is reserved. It moves `cash → in_kind` on the first in-kind mint
-- (the hub does that, not a trigger: the aggregate raises the audit fact) and back only
-- by an explicit operator command (`SetAllocationBacking`).
--
-- Backfill BY DATA, not by name: a product with at least one in-kind mint on record
-- (`unit_issuances.source = 'mint'`) is `in_kind`. In production that is `service_arb`
-- and `test_book`; a product seeded by subscriptions alone stays `cash`. A hand-over out
-- of the company's stake (`source = 'company'`) is not evidence on its own — the company
-- could only have been seeded by a mint, which the first predicate already catches.
--
-- (b) `unit_issuances.source` widens to `retire`: the mint's mirror, units burnt out of a
-- holder's account with no cash leg (`Dr shares_outstanding / Cr <holder shares>`).
-- `units` on a retire row is the MAGNITUDE — positive, like every other row — and the
-- source carries the direction, so the `^[1-9][0-9]*$` CHECK from 0034 is untouched and
-- the console reads one history. The CHECK on `source` was auto-named by 0038's inline
-- `CHECK (source IN (...))`; Postgres calls it `unit_issuances_source_check`, and it is
-- dropped and recreated under an explicit name so the next widening need not guess.
--
-- Backward compatible in both directions of a rolling deploy: a constant `DEFAULT` on
-- Postgres ≥ 11 adds the column without rewriting the table (a handful of rows either
-- way); a pod that predates the column inserts `allocations` without naming it and lands
-- on `cash`, and never selects it. Such a pod cannot write a `retire` row — it has no
-- code path that does — and reads issuance rows by id or by `(service, key)`, none of
-- which it holds for a retire, so the wider CHECK is invisible to it. No `lock_timeout`,
-- like every neighbouring migration: the tables are tiny and the DDL is instant.
--
-- Migrations run at hub boot. Reversible in part: `ALTER TABLE allocations DROP COLUMN
-- backing` loses nothing a pre-0039 binary can read. The `source` CHECK cannot be
-- narrowed back to ('mint', 'company') while a `retire` row exists — those rows are the
-- record of supply that was burnt, and deleting them would leave the ledger's
-- `shares_outstanding` unexplained; a rollback that needs the narrow CHECK must first
-- accept that the retire rows stay.
ALTER TABLE allocations
    ADD COLUMN backing TEXT NOT NULL DEFAULT 'cash' CHECK (backing IN ('cash', 'in_kind'));

COMMENT ON COLUMN allocations.backing IS
    'cash | in_kind — whether the fund''s claim holds the cash behind the units. Redeem is refused on in_kind (holders exit through the book). Flipped to in_kind by the first in-kind mint; back to cash only by SetAllocationBacking.';

UPDATE allocations
   SET backing = 'in_kind'
 WHERE service IN (SELECT DISTINCT service FROM unit_issuances WHERE source = 'mint');

ALTER TABLE unit_issuances
    DROP CONSTRAINT unit_issuances_source_check,
    ADD CONSTRAINT unit_issuances_source_check CHECK (source IN ('mint', 'company', 'retire'));

COMMENT ON COLUMN unit_issuances.source IS
    'mint | company | retire — minted in kind (Dr holder shares / Cr shares_outstanding), moved out of the company''s stake (Dr shares:<svc>:<user> / Cr shares_company:<svc>, supply unchanged), or retired (Dr shares_outstanding / Cr holder shares, supply shrinks). units is always the magnitude.';
