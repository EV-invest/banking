-- 0005: native_spend — the signer's own ledger of native coin each signature commits to
-- spending, so the spend policy can bound a (wallet, network) over a sliding window.
--
-- The fee budget (#182) bounds ONE signature; nothing bounded how many a compromised hub may
-- ask for in an hour, and on Tron there is no nonce to stop it (#369). Before a digest is
-- signed the handler records the native amount the transaction can burn or move — EVM
-- `gas_price × gas_limit` plus the native `value`, Tron `fee_limit` or the TRX amount, TON
-- `msg_value` or the Toncoin amount — and refuses when the window's total would exceed the
-- rail's `SIGNER_MAX_NATIVE_SPEND_PER_HOUR_*`. Check and insert run in one transaction under
-- `pg_advisory_xact_lock(hashtext(wallet_id), hashtext(network))`, so two concurrent requests
-- cannot both see the window as open. A row is written BEFORE the signature is asked for,
-- so a concurrent request sees the window as taken; if the request then fails before a
-- signature exists (the backend refuses, a later window is full) the handler deletes the row
-- again — a signed request keeps it whatever happens afterwards.
--
-- `spend` is NUMERIC(39,0): a `u128` wei amount (up to 3.4e38, 39 digits) does not fit a
-- BIGINT, and the signer binds it as text on the way in and reads the window's SUM back as
-- text, so no decimal crate is involved.
--
-- Rows older than the window are deleted by the same transaction that records a new one, per
-- (wallet, network) — the table only ever holds the live window plus whatever a quiet wallet
-- left behind.
--
-- Irreversible? No: purely additive, and the old binary neither reads nor writes it. A `down`
-- would be `DROP TABLE native_spend` — this repo's migrator carries no down files (see
-- 0001-0004), so the rollback path is "roll the image back and leave the table in place".
SET lock_timeout = '3s';

CREATE TABLE native_spend (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    wallet_id  UUID NOT NULL,
    network    TEXT NOT NULL,
    spend      NUMERIC(39, 0) NOT NULL CHECK (spend >= 0),
    signed_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The window query and the sweep both key on this; the table is new and empty, so a plain
-- CREATE INDEX holds no lock anyone is waiting on.
CREATE INDEX native_spend_wallet_network_signed_at ON native_spend (wallet_id, network, signed_at);
