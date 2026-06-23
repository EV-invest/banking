-- 0001: wallet_secrets — the signer's at-rest store of chain private keys.
--
-- This table lives in the SIGNER's own database (separate creds from the hub).
-- Each row is one (user, network) keypair: the private key is stored ONLY as the
-- sealed blob (XChaCha20-Poly1305 envelope, `nonce(24)||ciphertext+tag`, output of
-- Vault::seal); the plaintext key never touches Postgres. `public_key`/`address` are
-- watch-only metadata. `key_alg`/`key_version` tag the curve + sealing scheme so a
-- future KEK/format rotation can open old blobs.
--
-- SCOPE: at-rest encryption protects a stolen dump, NOT an RCE on the live signer.
-- Hot-float only — real balances belong behind MPC/HSM + an offline cold tier.

CREATE TABLE wallet_secrets (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL,
    network     TEXT NOT NULL,
    public_key  BYTEA NOT NULL,
    address     TEXT NOT NULL,
    sealed_key  BYTEA NOT NULL,
    key_alg     TEXT NOT NULL,
    key_version INTEGER NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One key per (user, network): the provisioning path is idempotent on this.
    UNIQUE (user_id, network)
);
