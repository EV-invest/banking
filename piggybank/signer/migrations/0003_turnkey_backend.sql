-- 0003: make room for a second key backend — EXPAND ONLY.
--
-- Phase 2 of docs/MIGRATION-turnkey.md. A row now says WHERE its private key lives:
--
--   backend='local'   — the key is the XChaCha20-Poly1305 blob in `sealed_key`,
--                       openable only under the KEK that sealed it (`kek_fp`).
--   backend='turnkey' — the key was minted inside a Turnkey enclave and NEVER
--                       existed in this process. There is no blob to store, which is
--                       the whole point; `turnkey_sign_with` is the account address
--                       Turnkey's `SignRawPayloadIntentV2.sign_with` takes, and it is
--                       the only handle the signing path needs.
--
-- Expand step ONLY — nothing is dropped, narrowed or renamed:
--   * `sealed_key` becomes nullable so a turnkey row can exist. Every EXISTING row
--     keeps its blob and its NOT NULL value; the CHECK below keeps a local row from
--     ever losing it.
--   * `backend` defaults to 'local', so every existing row reads back as exactly what
--     it is without a backfill pass.
--
-- Backward compatible on purpose: migrations run at signer startup (main.rs), so
-- during a rollout the PREVIOUS binary — which knows nothing of these columns — keeps
-- serving against this schema. It inserts with `sealed_key` set and no `backend`, the
-- default fills in 'local', and the CHECK passes. Rolling the image back is safe too.
--
-- The contract step (dropping `sealed_key`/`kek_fp` once every address has been
-- rotated onto Turnkey) is phase 5, NOT this migration: removing the KEK columns
-- while a single `backend='local'` row still holds funds strands them forever.
--
-- Irreversible? No: every change here is additive, so a `down` would be
-- `DROP COLUMN backend, turnkey_sign_with` + restore NOT NULL. This repo's migrator
-- (sqlx, applied on boot) carries no down files — see 0001/0002 — so the rollback
-- path is "roll the image back and leave the columns in place", which is safe
-- precisely because the expand step is backward compatible.

-- Fail fast rather than queueing behind a long-running reader and blocking the
-- signer's own traffic: a boot-time migration that waits is a boot that hangs.
SET lock_timeout = '3s';

ALTER TABLE wallet_secrets
    ALTER COLUMN sealed_key DROP NOT NULL;

-- Postgres 11+ stores a non-volatile column default in the catalogue, so adding a
-- NOT NULL DEFAULT column is a metadata-only change — no table rewrite, no long
-- exclusive lock, even though every existing row acquires a value.
ALTER TABLE wallet_secrets
    ADD COLUMN backend           TEXT NOT NULL DEFAULT 'local',
    ADD COLUMN turnkey_sign_with TEXT;

-- The invariant that replaces the NOT NULL we just dropped: each backend must carry
-- exactly the handle it can actually sign with. Without this, a bug could persist a
-- local row with no blob — an address that receives deposits nothing can ever move.
-- Validated immediately (not NOT VALID): the table holds one row per (user, network)
-- on a hot-float deposit book, and every existing row already satisfies it.
ALTER TABLE wallet_secrets
    ADD CONSTRAINT wallet_secrets_backend_handle CHECK (
        (backend = 'local'   AND sealed_key IS NOT NULL)
     OR (backend = 'turnkey' AND turnkey_sign_with IS NOT NULL)
    );
