-- 0022: the fee plane — 2% management + 20% performance, charged in units.
--
-- 0005 reserved `fund_positions.high_water_mark` "so adding a performance fee later
-- isn't a painful backfill once positions exist". This is that follow-up, and the
-- reservation paid off: the mark is already per-investor, which is what lets every
-- holder's performance fee be measured against their OWN entry price instead of one
-- fund-level mark that would make late subscribers subsidise early ones.
--
-- Control plane only — ZERO amounts reasoned about in SQL. Every figure here is an
-- exact integer count of 18-decimal base units stored as TEXT, and the fee itself is
-- moved by TigerBeetle (`Dr FeeShares / Cr UserShares` on the Share ledger). The tables
-- below hold policy, per-investor accrual clocks, and the audit trail.

-- (1) fee_policies: one fund's terms, keyed by the same `service` slug the allocation
-- registry owns. A product with NO row here charges nothing — the fee is opt-in per
-- product, so it can never appear on a fund whose prospectus did not promise it.
-- `basis` picks what the management fee is charged on: the investor's invested capital
-- (the default — the "static money" that actually went in, and a base the operator's own
-- AUM post cannot inflate) or the mark-to-market value of the holding.
CREATE TABLE fee_policies (
    service         TEXT PRIMARY KEY REFERENCES allocations (service) ON DELETE CASCADE,
    management_bps  INTEGER NOT NULL CHECK (management_bps BETWEEN 0 AND 10000),
    performance_bps INTEGER NOT NULL CHECK (performance_bps BETWEEN 0 AND 10000),
    hurdle_bps      INTEGER NOT NULL DEFAULT 0 CHECK (hurdle_bps BETWEEN 0 AND 10000),
    basis           TEXT NOT NULL CHECK (basis IN ('invested_capital', 'market_value')),
    crystallization TEXT NOT NULL CHECK (crystallization IN ('monthly', 'quarterly', 'semi_annual', 'annual')),
    updated_by      TEXT NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- (2) The per-investor accrual state, alongside the cost basis and mark already on the
-- position projection.
--
-- `fee_debt` is fee that was assessed but could not be collected — the holder's units
-- were locked by a queued redemption, or the charge floored below one base unit of
-- share. It is carried, never written off, and collected on the next assessment. It
-- exists so that "cannot collect" is a *deferral* rather than either a lost fee or a
-- negative balance; the investor's cash claim is never touched by a fee at all.
--
-- The two clocks are separate on purpose: management accrues continuously (so its clock
-- moves on every assessment) while performance crystallizes only at the end of a period
-- (so its clock moves only when the gain is actually locked in).
ALTER TABLE fund_positions
    ADD COLUMN fee_debt        TEXT NOT NULL DEFAULT '0' CHECK (fee_debt ~ '^[0-9]+$'),
    ADD COLUMN fees_accrued_at TIMESTAMPTZ,
    ADD COLUMN crystallized_at TIMESTAMPTZ;

-- Backfill the clocks from the position's own creation. Without this every existing
-- investor's first assessment would bill management fees back to the unix epoch.
UPDATE fund_positions SET fees_accrued_at = created_at WHERE fees_accrued_at IS NULL;
UPDATE fund_positions SET crystallized_at = created_at WHERE crystallized_at IS NULL;

ALTER TABLE fund_positions
    ALTER COLUMN fees_accrued_at SET NOT NULL,
    ALTER COLUMN crystallized_at SET NOT NULL,
    ALTER COLUMN fees_accrued_at SET DEFAULT now(),
    ALTER COLUMN crystallized_at SET DEFAULT now();

-- Backfill the reserved mark to each investor's average entry price
-- (`cost_basis / units`, at the canonical 18-dp scale). A mark left at the reserved '0'
-- would treat the WHOLE of an investor's NAV as profit on the first crystallization and
-- charge 20% of their capital — the single most dangerous thing this migration prevents.
-- Truncated, not rounded: the lower of two adjacent marks is the one that favours the
-- investor. Positions holding no units keep '0' and are re-marked at the NAV of their
-- next subscription (see the projection's blend below).
UPDATE fund_positions
SET high_water_mark = trunc(cost_basis::numeric * 1000000000000000000::numeric / units::numeric)::text
WHERE units <> '0' AND high_water_mark = '0';

-- The sweeper walks positions oldest-accrual-first; only unit-holding positions can be
-- charged, so the index is partial on exactly that set.
CREATE INDEX fund_positions_fee_due_idx ON fund_positions (fees_accrued_at) WHERE units <> '0';

-- (3) fee_assessments: the immutable audit trail — one row per charge against one
-- holding, and the read model behind the investor's fee statement. It records what was
-- owed and what was actually taken, which are not the same number whenever a charge
-- defers into `fee_debt`.
CREATE TABLE fee_assessments (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL,
    service         TEXT NOT NULL,
    trigger_kind    TEXT NOT NULL CHECK (trigger_kind IN ('period', 'redemption')),
    nav             TEXT NOT NULL CHECK (nav ~ '^[1-9][0-9]*$'),
    management      TEXT NOT NULL CHECK (management ~ '^[0-9]+$'),
    performance     TEXT NOT NULL CHECK (performance ~ '^[0-9]+$'),
    debt_opening    TEXT NOT NULL CHECK (debt_opening ~ '^[0-9]+$'),
    charged_units   TEXT NOT NULL CHECK (charged_units ~ '^[1-9][0-9]*$'),
    charged_cash    TEXT NOT NULL CHECK (charged_cash ~ '^[0-9]+$'),
    debt_carried    TEXT NOT NULL CHECK (debt_carried ~ '^[0-9]+$'),
    high_water_mark TEXT NOT NULL CHECK (high_water_mark ~ '^[0-9]+$'),
    assessed_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX fee_assessments_user_idx ON fee_assessments (user_id, assessed_at DESC);
CREATE INDEX fee_assessments_service_idx ON fee_assessments (service, assessed_at DESC);

-- (4) fee_settlements: the ONE place a fee crosses into cash. Charging is free — it
-- moves units between two holders on the Share ledger and no value leaves custody — so
-- the manager accumulates units and converts them in a single bulk operation per period
-- rather than paying a chain fee per investor. Burn the fee units, pay their value out
-- of the fund's claim into fee revenue.
CREATE TABLE fee_settlements (
    id         UUID PRIMARY KEY,
    service    TEXT NOT NULL,
    units      TEXT NOT NULL CHECK (units ~ '^[1-9][0-9]*$'),
    nav        TEXT NOT NULL CHECK (nav ~ '^[1-9][0-9]*$'),
    cash       TEXT NOT NULL CHECK (cash ~ '^[1-9][0-9]*$'),
    settled_by TEXT NOT NULL,
    settled_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX fee_settlements_service_idx ON fee_settlements (service, settled_at DESC);
