-- The money-plane operations mode: a global read-only kill-switch that pauses every
-- user money mutation (withdraw / subscribe / redeem) — the admin console's
-- "Read-only mode: pause deposits & withdrawals" toggle.
--
-- Singleton row (id always TRUE), mirroring the bridge_cursor pattern. Distinct from
-- the concierge maintenance/holding-page flag (that plane owns the cabinet chrome;
-- this plane owns money movement, so it enforces the pause).
CREATE TABLE operations_mode (
    id         BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    read_only  BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO operations_mode (id) VALUES (TRUE);
