-- 0021: the allocation registry — an investable product now has to be declared.
--
-- Until now a fund existed the instant anyone subscribed to a slug: `Subscribe`
-- validated only the slug's SHAPE (1..64 `[A-Za-z0-9_-]`) and a service with no
-- valuation silently bootstrapped at the seed NAV 1.0. So any investor could mint a
-- fund by typing an unregistered id — real money into a `service:<typo>` claim, with
-- no operator behind it. This table is the gate: the subscribe path resolves its
-- service here first, and only an `open` allocation takes new money.
--
-- Control plane only — ZERO amounts. Units, NAV, cost basis and cash stay where they
-- are (TigerBeetle + fund_valuations/fund_positions), keyed by this same `service`.

-- The `allocations` table created by 0002 was the write store of the ORIGINAL, since
-- abandoned per-service "allocation stake" aggregate (superseded by NAV/unit fund
-- shares in 0005). No code has read or written it since; dropping it frees the name
-- for the primitive that actually earned it.
DROP TABLE IF EXISTS allocations;

-- `service` is the natural key and the id every other context uses; `id` is a
-- surrogate UUID carried only so the aggregate satisfies the Entity/event-log
-- plumbing (`event_log.aggregate_id` is a UUID). Look rows up by `service`.
-- `state` is the domain's `AllocationState` discriminant — draft | open | closed.
CREATE TABLE allocations (
    id         UUID PRIMARY KEY,
    service    TEXT NOT NULL UNIQUE CHECK (service ~ '^[A-Za-z0-9_-]{1,64}$'),
    title      TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
    summary    TEXT NOT NULL DEFAULT '' CHECK (length(summary) <= 280),
    state      TEXT NOT NULL CHECK (state IN ('draft', 'open', 'closed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The default catalog is the `open` set; the partial index matches that read.
CREATE INDEX allocations_open_idx ON allocations (service) WHERE state = 'open';

-- Backfill every service that ALREADY carries money or history, as `open`. Without
-- this the gate would retroactively lock existing investors out of `Redeem` — units
-- already minted against an unregistered slug would have no way back to cash. The
-- title seeds to the slug; an operator renames it via UpdateAllocation.
INSERT INTO allocations (id, service, title, state)
SELECT gen_random_uuid(), service, service, 'open'
FROM (
    SELECT service FROM subscriptions
    UNION
    SELECT service FROM redemptions
    UNION
    SELECT service FROM fund_positions
    UNION
    SELECT service FROM fund_valuations
) AS known(service)
WHERE service ~ '^[A-Za-z0-9_-]{1,64}$'
ON CONFLICT (service) DO NOTHING;
