-- 0038: where an issuance's units come from — minted, or handed over out of the
-- company's stake.
--
-- `unit_issuances` (0034) recorded one shape: a mint with no cash leg, `Dr <holder shares>
-- / Cr shares_outstanding`. The company's stake in a product registered against an asset
-- it owns was seeded that way. There was no path back out of `shares_company:<svc>`: the
-- company has no user, so no book order and no escrow, and minting a user a second copy
-- of what the company holds would inflate the supply and break the cap table. This
-- column names the second path — `company`: the relay posts `Dr shares:<svc>:<user> /
-- Cr shares_company:<svc>` and `shares_outstanding` does not move. The row is otherwise
-- the same record (holder, units, the mark, the basis, the retry key, queued → applied)
-- and the recipient's `fund_positions` projection is the same, which is why it is a
-- column here and not a second table.
--
-- Backward compatible in both directions of a rolling deploy: a constant `DEFAULT` on
-- Postgres ≥ 11 adds the column without rewriting the table (two rows in production
-- either way), a pod that predates the column inserts without naming it and lands on
-- `mint` — which every row it can write IS — and never selects it. The paired CHECK
-- keeps the new value honest: the company hands units to a user, never to itself.
--
-- Migrations run at hub boot. Reversible: `ALTER TABLE unit_issuances DROP COLUMN source`
-- loses nothing a pre-0038 binary can read.
ALTER TABLE unit_issuances
    ADD COLUMN source TEXT NOT NULL DEFAULT 'mint' CHECK (source IN ('mint', 'company')),
    ADD CONSTRAINT unit_issuances_company_source_names_a_user CHECK (source <> 'company' OR holder_kind = 'user');

COMMENT ON COLUMN unit_issuances.source IS
    'mint | company — minted in kind (Dr holder shares / Cr shares_outstanding), or moved out of the company''s stake (Dr shares:<svc>:<user> / Cr shares_company:<svc>, supply unchanged).';
