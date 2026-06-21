-- 0005: fund shares — the service currency (NAV/unit accounting).
--
-- Adds the control plane for fund units on top of the money plane. TigerBeetle stays
-- authoritative for amounts: the user's unit holding and a fund's units outstanding
-- live on the new Share ledger (Ledger::Share); Postgres holds ONLY ids, saga state,
-- the per-mark valuation history, and a per-investor projection — never a second copy
-- of a unit balance. All money/unit/price values are exact integer counts of canonical
-- 18-decimal base units, stored as TEXT and parsed to u128 in the adapters.

-- (1) fund_valuations: append-only operator marks. NAV is *derived* — the operator
-- posts a fund's total AUM, and the handler reads units_outstanding live from TB at
-- that instant to compute nav = aum / units_outstanding (frozen until the next mark).
-- The "current nav" for a service is the latest row by posted_at. `posted_by` records
-- the operator subject (the trust seam). Subscribe/redeem read this; they never derive.
CREATE TABLE fund_valuations (
    id                UUID PRIMARY KEY,
    service           TEXT NOT NULL,
    aum               TEXT NOT NULL CHECK (aum ~ '^[0-9]+$'),
    units_outstanding TEXT NOT NULL CHECK (units_outstanding ~ '^[1-9][0-9]*$'),
    nav               TEXT NOT NULL CHECK (nav ~ '^[0-9]+$'),
    posted_by         TEXT NOT NULL,
    posted_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX fund_valuations_service_idx ON fund_valuations (service, posted_at DESC);

-- (2) subscriptions: the Subscription aggregate's write store + read projection. An
-- immutable mint record — `cash` of the user's claim bought `units` at `nav`. The cash
-- move and the unit mint live in TigerBeetle; this is the audit/projection row.
CREATE TABLE subscriptions (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL,
    service    TEXT NOT NULL,
    cash       TEXT NOT NULL CHECK (cash ~ '^[1-9][0-9]*$'),
    nav        TEXT NOT NULL CHECK (nav ~ '^[1-9][0-9]*$'),
    units      TEXT NOT NULL CHECK (units ~ '^[1-9][0-9]*$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX subscriptions_user_idx ON subscriptions (user_id);

-- (3) redemptions: the Redemption aggregate's write store + read projection. The
-- accept-and-queue saga's state (queued → completed | failed | cancelled) lives here;
-- the pending unit-burn and the cash payout live in TigerBeetle. `units` are fixed at
-- request; `nav`/`cash` are NULL until settle (settle-time pricing).
CREATE TABLE redemptions (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL,
    service    TEXT NOT NULL,
    units      TEXT NOT NULL CHECK (units ~ '^[1-9][0-9]*$'),
    nav        TEXT CHECK (nav ~ '^[1-9][0-9]*$'),
    cash       TEXT CHECK (cash ~ '^[1-9][0-9]*$'),
    state      TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX redemptions_user_idx ON redemptions (user_id);

-- (4) fund_positions: a per-(user, service) projection. Units are authoritative in TB;
-- this row carries the cost basis (average-cost net cash in) for P&L and is the lock
-- target that SERIALIZES concurrent redemptions (SELECT … FOR UPDATE) so two requests
-- can't both pass the Read-First check — though TigerBeetle's non-negative flag is the
-- actual money backstop. `high_water_mark` is reserved now (fee logic is out of scope
-- for v1) so adding a performance fee later isn't a painful backfill once positions exist.
CREATE TABLE fund_positions (
    user_id         UUID NOT NULL,
    service         TEXT NOT NULL,
    cost_basis      TEXT NOT NULL DEFAULT '0' CHECK (cost_basis ~ '^[0-9]+$'),
    high_water_mark TEXT NOT NULL DEFAULT '0' CHECK (high_water_mark ~ '^[0-9]+$'),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, service)
);
