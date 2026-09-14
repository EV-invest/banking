-- 0032: the allocation book — holders trading a product's units with each other.
--
-- Until now a unit changed hands only against the fund: a subscription minted it at NAV,
-- a redemption burned it at NAV. These four tables are the control plane of a secondary
-- market beside that — a central limit order book per allocation, price-time priority —
-- where two holders agree a price of their own. NAV is untouched: nothing here mints,
-- burns or revalues; a trade moves units and cash between two users through the ledger
-- (`Dr shares:<svc>:<buyer> / Cr book_shares:<svc>:<seller>` and `Dr book_cash:<buyer> /
-- Cr user:<seller>`, one linked TigerBeetle batch), and these rows are the record of who
-- asked for what and what matched.
--
-- Control plane only — ZERO authoritative amounts. Prices, sizes, notionals and fees are
-- exact 18-dp base-unit digit strings, the one representation every money column here
-- uses; the units and cash an order has committed live in the per-user escrow accounts
-- on the ledger. The SQL that does cast these columns (`::numeric`) does so to SORT the
-- resting side for the matcher, to AGGREGATE a depth snapshot and a candle, and to keep
-- the cost-basis projection honest — reads and projections, never a balance a payout is
-- decided on. That is the same latitude `fund_positions` already takes.
--
-- Matching runs under one transaction-scoped advisory lock per book
-- (`pg_advisory_xact_lock(hashtext(service))`), so the book has exactly one writer at a
-- time and the resting side the matcher reads is the whole truth. `seq` — not
-- `created_at` — is the time in price-TIME priority: it is assigned at insert, under
-- that lock, so it orders arrivals the way the lock serialized them, where `now()` is
-- the transaction's START and two transactions that queued on the lock could carry it
-- in the wrong order.
--
-- The book is opt-in per product, like the fee: no `book_policies` row means a closed
-- book. `book_revisions` is the counter every change bumps in its own transaction; the
-- live feed is keyed on it and a client refetches when it moves.
--
-- All four tables are new and empty, so no lock or backfill concern applies; migrations
-- run at hub boot, as every migration here does. No `ON DELETE` on any reference:
-- allocations and users are never deleted, and an order or a trade must outlive nothing.

CREATE TABLE book_policies (
    service             TEXT        PRIMARY KEY REFERENCES allocations (service),
    book_open           BOOLEAN     NOT NULL DEFAULT false,
    taker_fee_bps       INTEGER     NOT NULL DEFAULT 0 CHECK (taker_fee_bps BETWEEN 0 AND 10000),
    -- 0.01 USDT and 0.0001 units, in 18-dp base units — the grid a product trades on
    -- until an operator says otherwise. Positive: a zero grid admits every price.
    price_tick          TEXT        NOT NULL DEFAULT '10000000000000000' CHECK (price_tick ~ '^[1-9][0-9]*$'),
    lot_size            TEXT        NOT NULL DEFAULT '100000000000000' CHECK (lot_size ~ '^[1-9][0-9]*$'),
    market_slippage_bps INTEGER     NOT NULL DEFAULT 500 CHECK (market_slippage_bps BETWEEN 0 AND 10000),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE book_policies IS
    'Per-allocation trading terms, set by AllocationManage. No row = closed book. Amounts are 18-dp base-unit digit strings.';
COMMENT ON COLUMN book_policies.taker_fee_bps IS
    'Basis points of a fill''s notional the TAKER pays into fee revenue, in the same linked batch as the fill. Makers pay nothing.';
COMMENT ON COLUMN book_policies.market_slippage_bps IS
    'How far past the best opposite quote a market order may fill: its limit is best × (1 ± this), rounded outward to the tick.';

CREATE TABLE book_orders (
    id              UUID        PRIMARY KEY,
    seq             BIGINT      GENERATED ALWAYS AS IDENTITY,
    service         TEXT        NOT NULL REFERENCES allocations (service),
    user_id         UUID        NOT NULL REFERENCES users (id),
    client_order_id TEXT        NOT NULL CHECK (char_length(client_order_id) BETWEEN 1 AND 64),
    side            TEXT        NOT NULL CHECK (side IN ('buy', 'sell')),
    kind            TEXT        NOT NULL CHECK (kind IN ('limit', 'market')),
    tif             TEXT        NOT NULL CHECK (tif IN ('gtc', 'ioc', 'alo')),
    -- A market order stores the limit the hub derived for it, so every row has a price.
    price           TEXT        NOT NULL CHECK (price ~ '^[1-9][0-9]*$'),
    size            TEXT        NOT NULL CHECK (size ~ '^[1-9][0-9]*$'),
    filled          TEXT        NOT NULL DEFAULT '0' CHECK (filled ~ '^[0-9]+$'),
    notional_filled TEXT        NOT NULL DEFAULT '0' CHECK (notional_filled ~ '^[0-9]+$'),
    fee_paid        TEXT        NOT NULL DEFAULT '0' CHECK (fee_paid ~ '^[0-9]+$'),
    -- What the order committed to escrow on placement: units (= size) for a sell, cash
    -- (notional at the limit plus the taker fee on it) for a buy. What is left of it at
    -- the end — reserved − notional_filled − fee_paid for a buy, size − filled for a
    -- sell — is what the release hands back.
    reserved        TEXT        NOT NULL CHECK (reserved ~ '^[1-9][0-9]*$'),
    state           TEXT        NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'partially_filled', 'filled', 'cancelled', 'rejected')),
    reject_reason   TEXT,
    -- Why a cancelled order ended: the owner's own cancel, or the hub cancelling the
    -- remainder an IOC limit / a market order could not fill at once. Without it a
    -- partially filled IOC (`cancelled`, `filled > 0`) is indistinguishable from a
    -- user's cancel after a partial fill.
    cancel_reason   TEXT        CHECK (cancel_reason IN ('user', 'ioc_remainder', 'market_remainder')),
    -- The book revision at which this row last changed — what WatchBook's
    -- `orders_revision` reports for the caller, so a client refetches its own orders only
    -- when one of them moved, not on every tick of the book.
    revision        BIGINT      NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((state = 'rejected') = (reject_reason IS NOT NULL)),
    CHECK ((state = 'cancelled') = (cancel_reason IS NOT NULL)),
    UNIQUE (user_id, client_order_id)
);

-- The matcher's read: one side of one book, best price first, oldest first within a
-- price. Partial, because only resting orders are ever matched and the resting set is a
-- small fraction of the table once history accumulates. Sorted on the numeric value, not
-- the digit string — '9' sorts after '10' as text.
CREATE INDEX book_orders_resting_idx ON book_orders (service, side, (price::numeric), seq)
    WHERE state IN ('open', 'partially_filled');
-- A user's own orders, newest first (open and history reads).
CREATE INDEX book_orders_user_idx ON book_orders (user_id, seq DESC);

COMMENT ON TABLE book_orders IS
    'Orders on the allocation book. The escrow they commit lives in TigerBeetle (book_shares / book_cash); this is the order itself, its fills and its state.';
COMMENT ON COLUMN book_orders.seq IS
    'Arrival order under the book''s write lock — the TIME in price-time priority. created_at is the transaction start and may disagree with it.';
COMMENT ON COLUMN book_orders.reserved IS
    'Units (sell) or 18-dp USDT (buy) moved to escrow on placement. The unspent part is released when the order ends.';
COMMENT ON COLUMN book_orders.state IS
    'open | partially_filled (resting) | filled | cancelled | rejected. rejected is written by the relay when the ledger refuses the escrow.';
COMMENT ON COLUMN book_orders.cancel_reason IS
    'Set exactly on cancelled: user (the owner''s cancel) | ioc_remainder | market_remainder (the hub cancelled what could not fill at once).';

CREATE TABLE book_trades (
    id            UUID        PRIMARY KEY,
    seq           BIGINT      GENERATED ALWAYS AS IDENTITY,
    service       TEXT        NOT NULL REFERENCES allocations (service),
    buyer_id      UUID        NOT NULL REFERENCES users (id),
    seller_id     UUID        NOT NULL REFERENCES users (id),
    buy_order_id  UUID        NOT NULL REFERENCES book_orders (id),
    sell_order_id UUID        NOT NULL REFERENCES book_orders (id),
    taker_side    TEXT        NOT NULL CHECK (taker_side IN ('buy', 'sell')),
    price         TEXT        NOT NULL CHECK (price ~ '^[1-9][0-9]*$'),
    size          TEXT        NOT NULL CHECK (size ~ '^[1-9][0-9]*$'),
    notional      TEXT        NOT NULL CHECK (notional ~ '^[0-9]+$'),
    fee           TEXT        NOT NULL CHECK (fee ~ '^[0-9]+$'),
    executed_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The matcher refuses a self-trade; the column agrees.
    CHECK (buyer_id <> seller_id)
);

-- The tape and the candles: one product's trades by time. `seq` breaks ties inside one
-- placement, whose fills all carry the transaction's `now()`.
CREATE INDEX book_trades_tape_idx ON book_trades (service, executed_at DESC, seq DESC);
-- A user's own fills, from either side.
CREATE INDEX book_trades_buyer_idx ON book_trades (buyer_id, seq DESC);
CREATE INDEX book_trades_seller_idx ON book_trades (seller_id, seq DESC);

COMMENT ON TABLE book_trades IS
    'Fills on the allocation book: the two orders, the two parties, the maker''s price, and the fee the taker paid. Settled on the ledger by the relay as one linked batch.';
COMMENT ON COLUMN book_trades.fee IS
    '18-dp USDT the TAKER paid into fee revenue on this fill; the maker paid nothing.';

CREATE TABLE book_revisions (
    service    TEXT        PRIMARY KEY REFERENCES allocations (service),
    revision   BIGINT      NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE book_revisions IS
    'One counter per book, bumped in the transaction of every placement, fill and cancel. WatchBook frames carry it; a client refetches when it moves.';
