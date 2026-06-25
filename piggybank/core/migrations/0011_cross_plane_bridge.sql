-- Cross-plane bridge state: the fields the one-way concierge→banking lifecycle
-- consumer applies, plus its delivery cursor.
--
-- Identity is OWNED by the concierge plane; banking only mirrors the slice it needs
-- to gate money ops. The correlation key is `auth_subject` — the provider (Google)
-- `sub` both planes provision against — already UNIQUE on `users` (migration 0001),
-- which a CREATED event provisions a minimal local row against.

-- `frozen` gates money-moving RPCs (SUSPENDED → true, REINSTATED → false).
-- `kyc_level` mirrors the concierge KYC tier (KYC_CHANGED).
-- `concierge_token_version` is the coarse revoke FLOOR carried by SESSIONS_REVOKED.
-- `last_lifecycle_sequence` is the per-user order guard: an event applies only if its
-- `sequence` exceeds this, so a stale REINSTATED can't un-freeze a later SUSPENDED.
ALTER TABLE users
    ADD COLUMN frozen                  BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN kyc_level               INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN concierge_token_version BIGINT  NOT NULL DEFAULT 0,
    ADD COLUMN last_lifecycle_sequence BIGINT  NOT NULL DEFAULT 0;

-- The consumer's delivery cursor: a single row holding the last consumed global
-- outbox `position`. CHECK pins it to one row so the cursor is unambiguous. The
-- consumer advances `position` only after a pulled batch is fully applied
-- (at-least-once + idempotent), so a crash mid-batch re-pulls and re-applies safely.
CREATE TABLE bridge_cursor (
    id         BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    position   BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO bridge_cursor (id) VALUES (TRUE);
