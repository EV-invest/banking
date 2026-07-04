-- 0002: KEK-epoch safety — the monument to the stranded-deposit bug class.
--
-- A key sealed under one KEK is unrecoverable under any other. Once a real deposit
-- landed on an address whose key was sealed under a since-lost ephemeral KEK, and the
-- loss surfaced only at withdrawal time. These structures make a KEK mismatch
-- IMPOSSIBLE to miss:
--
--   kek_sentinel  — one row sealing a known plaintext under the boot KEK. On every
--                   boot the signer must unseal it (and match the fingerprint) or it
--                   refuses to serve: a whole-database epoch mismatch dies at startup,
--                   never lazily at sign time.
--   kek_fp        — the fingerprint (domain-separated SHA-256 of the KEK) stamped on
--                   each wallet_secrets row at seal time. A row whose fp differs from
--                   the boot KEK's is PROVABLY dead and surfaces in the key-health
--                   diagnostics instead of failing on the sweep/withdrawal path.
--                   NULL only for pre-epoch rows until the boot backfill probes them.
--   superseded_at — archives a dead row so a rotation can mint a replacement key for
--                   the same (user, network). Uniqueness moves to a partial index over
--                   the ACTIVE row; superseded rows are kept forever for forensics.

CREATE TABLE kek_sentinel (
    -- Single-row table: the fixed TRUE key makes a second epoch row unrepresentable.
    id           BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    kek_fp       BYTEA NOT NULL,
    sealed_probe BYTEA NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE wallet_secrets
    ADD COLUMN kek_fp BYTEA,
    ADD COLUMN superseded_at TIMESTAMPTZ;

ALTER TABLE wallet_secrets DROP CONSTRAINT wallet_secrets_user_id_network_key;
CREATE UNIQUE INDEX wallet_secrets_active_user_network
    ON wallet_secrets (user_id, network) WHERE superseded_at IS NULL;
