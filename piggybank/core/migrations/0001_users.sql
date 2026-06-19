-- The hub's first migration: the `users` control plane + the domain event log +
-- the UUID↔TigerBeetle id-map. Money lives ONLY in TigerBeetle; no amount column
-- exists anywhere here.

-- Investor identity. `auth_subject` (Google's immutable `sub`) is the UNIQUE
-- provisioning key; `email` is NOT unique (a person may change it behind a stable
-- subject). `token_version` backs coarse "revoke all". Audit timestamps are
-- DB-managed (not modelled on the aggregate).
CREATE TABLE users (
    id            UUID PRIMARY KEY,
    auth_subject  TEXT NOT NULL UNIQUE,
    email         TEXT NOT NULL,
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    status        TEXT NOT NULL DEFAULT 'active',
    token_version BIGINT NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Append-only domain event log: the audit trail and the source a future read
-- projection replays from. The transactional OUTBOX + relay land with the first
-- TigerBeetle-crossing or multi-aggregate write (see
-- piggybank/core/src/infrastructure/outbox.rs); a single-aggregate identity write
-- needs only this log, written in the same transaction as the state change.
CREATE TABLE event_log (
    seq          BIGSERIAL PRIMARY KEY,
    aggregate    TEXT NOT NULL,
    aggregate_id UUID NOT NULL,
    kind         TEXT NOT NULL,
    payload      JSONB NOT NULL,
    occurred_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX event_log_aggregate_idx ON event_log (aggregate, aggregate_id);

-- UUID → TigerBeetle account id map. Stores the id mapping only — ZERO amounts.
-- `tb_account_id` is the u128 ledger account id as 16 big-endian bytes. Populated
-- lazily by a future money slice; empty today, so `GetBalance` reads as 0 without
-- touching the ledger.
CREATE TABLE ledger_accounts (
    user_id       UUID PRIMARY KEY REFERENCES users (id),
    tb_account_id BYTEA NOT NULL,
    ledger        INTEGER NOT NULL,
    code          INTEGER NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
