-- 0031: units issued in kind — an operator minting fund units with no cash behind them.
--
-- Until now every unit came out of `subscriptions` (0005): cash left a user's claim,
-- units arrived in their holding, one row per mint. That has no honest shape for a
-- product registered against an asset the company already owns, where the supply is
-- fixed at registration and the company and a named investor hold their shares because
-- they own the asset, not because they wired cash into a fund claim. This table is the
-- control-plane record of such a mint; the units themselves are minted by the relay on
-- the Share ledger (`Dr shares:<svc>:<user> | shares_company:<svc> / Cr
-- shares_outstanding:<svc>`), so the supply invariant holds by construction and the
-- issued units count against the allocation's cap exactly as a subscription's do.
--
-- Control plane only — ZERO amounts reasoned about in SQL. `units`, `nav` and
-- `cost_basis` are exact 18-dp base-unit digit strings, the one representation every
-- money column here uses. `cost_basis` may be zero (an asset the company already owned
-- cost it no cash), where `units` and `nav` may not.
--
-- The holder is a `(kind, id)` pair rather than a nullable user id alone, mirroring the
-- `Party` columns of 0002: `company` is a holder in its own right, with no `users` row
-- and no position projection, and a synthetic user standing in for it would drag every
-- investor-facing read into special-casing one UUID. The paired CHECK keeps the two
-- columns honest in both directions. `holder_id` references `users` because the domain
-- rule is that the investor must exist — a mint to a UUID nobody can sign in as is
-- money nobody can redeem.
--
-- `(service, idempotency_key)` is the retry contract: an admin console that re-sends a
-- timed-out request lands on the conflict and is handed the existing row, never a
-- second mint. The key is per product so two operators sizing two products need not
-- coordinate their key spaces.
--
-- `state` moves `queued` → `applied` exactly once, written by the relay after the mint
-- posts, alongside `applied_at` — the paired CHECK makes a state without its timestamp
-- (or the reverse) unrepresentable.
--
-- No `ON DELETE` on either reference: allocations and users are never deleted.
CREATE TABLE unit_issuances (
    id              UUID        PRIMARY KEY,
    service         TEXT        NOT NULL REFERENCES allocations (service),
    holder_kind     TEXT        NOT NULL CHECK (holder_kind IN ('user', 'company')),
    holder_id       UUID        REFERENCES users (id),
    units           TEXT        NOT NULL CHECK (units ~ '^[1-9][0-9]*$'),
    nav             TEXT        NOT NULL CHECK (nav ~ '^[1-9][0-9]*$'),
    cost_basis      TEXT        NOT NULL CHECK (cost_basis ~ '^[0-9]+$'),
    idempotency_key TEXT        NOT NULL CHECK (char_length(idempotency_key) BETWEEN 1 AND 64),
    state           TEXT        NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'applied')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    applied_at      TIMESTAMPTZ,
    CHECK ((holder_kind = 'user') = (holder_id IS NOT NULL)),
    CHECK ((state = 'applied') = (applied_at IS NOT NULL)),
    UNIQUE (service, idempotency_key)
);

-- The operator's per-product history, newest first.
CREATE INDEX unit_issuances_service_idx ON unit_issuances (service, created_at DESC);

COMMENT ON TABLE unit_issuances IS
    'In-kind unit mints (no cash leg) by an AllocationManage holder. The units live in TigerBeetle; this row is the record, the retry key and the queued/applied state.';
COMMENT ON COLUMN unit_issuances.holder_kind IS
    'user | company — who the units were minted to. `company` is the fund''s own stake (shares_company:<service>), not a user.';
COMMENT ON COLUMN unit_issuances.cost_basis IS
    '18-dp USDT base units the holder is deemed to have paid. Defaults to units × nav; an explicit zero is legal. Added to fund_positions.cost_basis for a user holder.';
COMMENT ON COLUMN unit_issuances.idempotency_key IS
    'Operator-supplied retry key, unique per service. A repeat returns this row instead of minting again.';
COMMENT ON COLUMN unit_issuances.state IS
    'queued until the relay posts the mint, then applied — written once, with applied_at.';
