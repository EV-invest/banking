-- 0027: an allocation now carries the glyph its catalog card is drawn with.
--
-- Until now the cabinet had nothing to draw a product with, so it fell back to a letter
-- avatar — a circle holding the first character of the title ("Q" for Quy Nhon Fund).
-- That was the absence of a decision, not a decision. This column is the decision: an
-- operator picks one value out of a closed set when registering or editing a product,
-- and the client maps it onto one of the SVGs it ships.
--
-- Presentation only, like `title` and `summary` — it gates neither money nor lifecycle,
-- and no read path branches on it. The storage adapter enforces that literally: a value
-- it cannot parse degrades to the default with a `warn!` rather than failing the read,
-- so this column can never take `find`/`list` — and with them subscribe and redeem —
-- down. Strict parsing stays on the gRPC boundary, where the input is a client's and can
-- still be refused.

-- A CHECK-constrained TEXT rather than `CREATE TYPE ... AS ENUM`, matching every other
-- vocabulary column in this schema (`allocations.state` in 0021, `withdrawals.source` in
-- 0024): widening the set is then an ordinary migration instead of an `ALTER TYPE ... ADD
-- VALUE`, which cannot run in a transaction alongside other statements.
--
-- The list is `domain::allocations::AllocationIcon::ALL`, and it is held to this one from
-- both sides: `domain_icons_match_the_wire_contract` compares the domain enum with
-- `evbanking_contracts::allocation::icon::ALL` member for member, and
-- `every_icon_the_domain_knows_is_accepted_by_the_column` writes every domain variant
-- into this column and asserts the CHECK takes it. Adding an icon therefore means four
-- edits: the enum (its `next` chain makes the compiler insist), `icon::ALL`, the client's
-- artwork, and a migration widening this list. Forget the migration and that integration
-- test is red — rather than an operator discovering it as a CHECK violation that rolls
-- back the whole `update_details`, taking their title edit with it and answering
-- `internal` where a validation error belonged.
--
-- `NOT NULL DEFAULT` in a single `ADD COLUMN` does not rewrite the table on PG11+ (the
-- default is recorded in the catalog and materialised on read) — the same shape 0022 and
-- 0024 used. It is NOT lock-free, though: the CHECK added in the same statement is
-- verified against existing rows, so the table is scanned once under ACCESS EXCLUSIVE and
-- everything else queues behind it. `allocations` holds one row per product, so that scan
-- is measured in microseconds — do not copy this shape onto a large table without either
-- splitting it (ADD COLUMN, then ADD CONSTRAINT ... NOT VALID, then VALIDATE CONSTRAINT,
-- which takes only SHARE UPDATE EXCLUSIVE) or accepting the outage.
--
-- It is readable by the CURRENTLY DEPLOYED code: its INSERT names its columns and its
-- UPDATE names its SET list, so a row it writes simply takes the default
-- (`a_row_written_by_a_pod_that_predates_the_icon_column_reads_back_as_the_default`).
ALTER TABLE allocations
    ADD COLUMN icon TEXT NOT NULL DEFAULT 'fund'
        CHECK (icon IN ('fund', 'real_estate', 'trading', 'yield', 'venture', 'treasury', 'commodity', 'credit', 'index', 'arbitrage'));

-- `fund` is `AllocationIcon::default()` — the neutral glyph. It is what every product
-- 0021 backfilled is showing today, so the default backfills to exactly what operators
-- already see rather than silently restyling the live catalog.
COMMENT ON COLUMN allocations.icon IS
    'Catalog glyph the client maps onto one of its SVGs. Presentation only — gates neither money nor lifecycle.';
