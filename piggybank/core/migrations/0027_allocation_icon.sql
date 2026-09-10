-- 0027: an allocation now carries the glyph its catalog card is drawn with.
--
-- Until now the cabinet had nothing to draw a product with, so it fell back to a letter
-- avatar — a circle holding the first character of the title ("Q" for Quy Nhon Fund).
-- That was the absence of a decision, not a decision. This column is the decision: an
-- operator picks one value out of a closed set when registering or editing a product,
-- and the client maps it onto one of the SVGs it ships.
--
-- Presentation only, like `title` and `summary` — it gates neither money nor lifecycle,
-- and no read path branches on it.

-- A CHECK-constrained TEXT rather than `CREATE TYPE ... AS ENUM`, matching every other
-- vocabulary column in this schema (`allocations.state` in 0021, `withdrawals.source` in
-- 0024): widening the set is then an ordinary migration instead of an `ALTER TYPE ... ADD
-- VALUE`, which cannot run in a transaction alongside other statements. The list is
-- `domain::allocations::AllocationIcon`; `domain_icons_match_the_wire_contract` keeps the
-- domain, the wire contract and this list from drifting apart.
--
-- `NOT NULL DEFAULT` in a single `ADD COLUMN` does not rewrite the table on PG11+ (the
-- default is recorded in the catalog and materialised on read), so this is a
-- metadata-only change holding no long lock — the same shape 0022 and 0024 used. It is
-- also readable by the CURRENTLY DEPLOYED code: its INSERT names its columns and its
-- UPDATE names its SET list, so a row it writes simply takes the default.
ALTER TABLE allocations
    ADD COLUMN icon TEXT NOT NULL DEFAULT 'fund'
        CHECK (icon IN ('fund', 'real_estate', 'trading', 'yield', 'venture', 'treasury', 'commodity', 'credit', 'index', 'arbitrage'));

-- `fund` is `AllocationIcon::default()` — the neutral glyph. It is what every product
-- 0021 backfilled is showing today, so the default backfills to exactly what operators
-- already see rather than silently restyling the live catalog.
COMMENT ON COLUMN allocations.icon IS
    'Catalog glyph the client maps onto one of its SVGs. Presentation only — gates neither money nor lifecycle.';
