-- 0002: the money plane — balance + allocations control-plane tables.
--
-- TigerBeetle stays the authoritative store of amounts (the data plane); Postgres
-- holds ONLY ids, state, the event log/outbox, and read projections — never a
-- second copy of a balance. USDT amounts are exact integer counts of canonical
-- 18-decimal base units, stored as TEXT (no float, no decimal-crate dependency;
-- the codebase already moves amounts as strings) and parsed to u128 in the adapter.

-- (1) Stable event id on the append-only log, and the transactional OUTBOX.
-- The relay derives deterministic TigerBeetle transfer ids from `event_id`, never
-- from the delivery cursor `seq` (a retried command must not mint a new transfer
-- id). Backfill any pre-existing rows (dev only) so the column can be NOT NULL.
ALTER TABLE event_log ADD COLUMN event_id UUID;
UPDATE event_log SET event_id = gen_random_uuid() WHERE event_id IS NULL;
ALTER TABLE event_log ALTER COLUMN event_id SET NOT NULL;
ALTER TABLE event_log ADD CONSTRAINT event_log_event_id_key UNIQUE (event_id);

-- Drained by the relay in strict `seq` order (single worker); `event_id` is the
-- idempotency key. `dispatched_at IS NULL` ⇒ not yet applied to the ledger.
CREATE TABLE outbox (
    seq           BIGSERIAL PRIMARY KEY,
    event_id      UUID NOT NULL UNIQUE,
    aggregate     TEXT NOT NULL,
    aggregate_id  UUID NOT NULL,
    kind          TEXT NOT NULL,
    payload       JSONB NOT NULL,
    occurred_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    dispatched_at TIMESTAMPTZ,
    attempts      INTEGER NOT NULL DEFAULT 0,
    last_error    TEXT
);
CREATE INDEX outbox_undispatched_idx ON outbox (seq) WHERE dispatched_at IS NULL;

-- (2) Generic logical-key → TigerBeetle u128 account-id map. Supersedes the empty
-- single-purpose `ledger_accounts`. Stores the id mapping + the account's
-- ledger/code/network only — ZERO amounts. `tb_account_id` is the u128 id as 16
-- big-endian bytes (same convention as the old table / `u128_from_be`).
DROP TABLE ledger_accounts;
CREATE TABLE tb_accounts (
    logical_key   TEXT PRIMARY KEY,
    tb_account_id BYTEA NOT NULL UNIQUE,
    ledger        INTEGER NOT NULL,
    code          INTEGER NOT NULL,
    network       TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- (3) allocations: the Allocation aggregate's write store AND read projection. The
-- per-allocation breakdown (who, how much, to where, state) lives here; TigerBeetle
-- holds only the net per-account balances. `owner_*` + `sharers` carry the
-- ownership model; `user_id`/`service_id` are denormalized for the "my allocations"
-- and per-service queries. Single-transition updates are guarded by the row lock +
-- `WHERE state = $expected` in the adapter.
CREATE TABLE allocations (
    id          UUID PRIMARY KEY,
    amount      TEXT NOT NULL CHECK (amount ~ '^[1-9][0-9]*$'),
    network     TEXT NOT NULL,
    owner_kind  TEXT NOT NULL,
    owner_id    TEXT,
    sharers     JSONB NOT NULL,
    kind        JSONB NOT NULL,
    user_id     UUID,
    service_id  TEXT,
    state       TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX allocations_user_idx ON allocations (user_id) WHERE user_id IS NOT NULL;
CREATE INDEX allocations_service_idx ON allocations (service_id) WHERE service_id IS NOT NULL;

-- (4) deposits: the idempotency GATE for recording on-chain deposits. The `tx_ref`
-- PRIMARY KEY makes a second record of the same chain tx impossible, so the deposit
-- event (and its ledger credit) happens at most once.
CREATE TABLE deposits (
    tx_ref     TEXT PRIMARY KEY,
    party_kind TEXT NOT NULL,
    party_id   TEXT,
    network    TEXT NOT NULL,
    amount     TEXT NOT NULL CHECK (amount ~ '^[1-9][0-9]*$'),
    event_id   UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- (5) saga_steps: a pure audit/forensics table recording the deterministic
-- TigerBeetle transfer id each relay event leg mapped to (write-only — the relay
-- inserts after a successful leg and never reads it back). Idempotency rests on the
-- determinism of `tid()` plus TigerBeetle's `Exists`, not on this row.
-- `(event_id, leg)` is the natural key.
CREATE TABLE saga_steps (
    event_id       UUID NOT NULL,
    leg            INTEGER NOT NULL,
    role           TEXT NOT NULL,
    tb_transfer_id BYTEA NOT NULL UNIQUE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (event_id, leg)
);

-- (6) fund_wallets: the fund's on-chain custody wallet metadata per network (the
-- address USDT is received at). Metadata only — the liquid balance is authoritative
-- in the TigerBeetle `wallet:<network>` custody account.
CREATE TABLE fund_wallets (
    network    TEXT PRIMARY KEY,
    token      TEXT NOT NULL,
    address    TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
