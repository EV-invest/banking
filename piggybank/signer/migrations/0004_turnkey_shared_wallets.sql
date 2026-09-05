-- 0004: shared per-network Turnkey wallets + a derivation index per account — EXPAND ONLY.
--
-- Corrects a design error in 0003/phase 2, before any Turnkey wallet has been created in
-- prod (KEY_BACKEND still defaults to `local`). 0003 modeled one Turnkey WALLET per
-- (user, network). Turnkey's org limits are:
--
--   HD Wallets            100        <- WALLETS are capped
--   HD Wallet Accounts    unlimited  <- ACCOUNTS are not
--   Sub-Organizations     unlimited
--
-- One wallet per (user, network) hits the 100-wallet ceiling at 25 users * 4 networks — and
-- it is the exact shape Turnkey's own model says not to use: a wallet is a seed, an account is
-- a derivation path off that seed, and paths are unlimited. The fix is one wallet PER NETWORK
-- (4 total, nowhere near the cap) and one ACCOUNT per user inside it, distinguished only by
-- `derivation_index`.
--
-- Two additive pieces:
--
--   * `wallet_secrets.derivation_index` — the per-account path component (see `turnkey.rs`'s
--     `derivation_path`). Nullable: a `backend='local'` row has no Turnkey account and so no
--     index; the CHECK below is what actually enforces "every turnkey row has one", the same
--     pattern 0003 used for `sealed_key`/`turnkey_sign_with`.
--   * `turnkey_network_wallets` — the 4-row registry mapping a network to the ONE Turnkey
--     wallet_id every user's account for that network lives in. Looked up before minting a new
--     account and populated lazily on a network's first-ever provision call.
--
-- `derivation_index` is drawn from a dedicated SEQUENCE, not a per-row IDENTITY: the signer
-- needs the index BEFORE the row can be inserted, to build the derivation path it asks Turnkey
-- to create the account at — the row is only written once Turnkey has confirmed that account
-- exists. A losing racer's pulled index is simply abandoned; the sequence's only job is to
-- never hand out the same value twice, not to avoid gaps. The provisioning idempotency check
-- (an existing row short-circuits before any index is pulled) means a repeated
-- provision(user, network) never touches the sequence at all, so this cannot make a stable
-- address unstable.
--
-- Irreversible? No: both the column and the table are purely additive. A `down` would be
-- `DROP TABLE turnkey_network_wallets`, `ALTER TABLE wallet_secrets DROP COLUMN
-- derivation_index`, `DROP SEQUENCE wallet_secrets_derivation_index_seq` — this repo's
-- migrator carries no down files (see 0001-0003), so the rollback path is "roll the image
-- back and leave the addition in place", safe because nothing here changes what an
-- unaware old binary reads or writes.
SET lock_timeout = '3s';

CREATE SEQUENCE wallet_secrets_derivation_index_seq AS BIGINT MINVALUE 0 START WITH 0;

-- Metadata-only on Postgres 11+ (no table rewrite): a nullable column with no default.
ALTER TABLE wallet_secrets
    ADD COLUMN derivation_index BIGINT;

-- Cosmetic, not load-bearing: ties the sequence's lifecycle to the column it fills so
-- `pg_depend` reflects the relationship, the same way a `BIGSERIAL` would.
ALTER SEQUENCE wallet_secrets_derivation_index_seq OWNED BY wallet_secrets.derivation_index;

-- The invariant that keeps a Turnkey row from ever mapping to nothing: exactly the same
-- shape as `wallet_secrets_backend_handle` in 0003, extended to the new column. Validated
-- immediately, not NOT VALID — the table is tiny and every existing row is `backend='local'`
-- with `derivation_index IS NULL`, which trivially satisfies it.
ALTER TABLE wallet_secrets
    ADD CONSTRAINT wallet_secrets_turnkey_has_index CHECK (
        backend <> 'turnkey' OR derivation_index IS NOT NULL
    );

-- One row per network, holding the single Turnkey wallet_id every user's account for that
-- network is created in. A concurrent double-bootstrap on a network's very first provision
-- call can mint two Turnkey wallets before this table has a row to short-circuit on; the
-- loser's INSERT is a no-op (see `ON CONFLICT DO NOTHING` in `secrets.rs`) and its wallet sits
-- unused — the same acceptable, bounded waste 0003's per-user design already tolerated for an
-- orphaned per-user wallet, except now it can happen at most 4 times total (once per network)
-- instead of once per user.
CREATE TABLE turnkey_network_wallets (
    network    TEXT PRIMARY KEY,
    wallet_id  TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
