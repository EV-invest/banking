-- The chain custody adapter's idempotency + crash-safety record for on-chain withdrawal
-- broadcasts. The signed transaction is persisted here BEFORE it is sent, so a retried
-- relay delivery (at-least-once) re-broadcasts the SAME bytes (same nonce) instead of
-- signing a new one — a withdrawal can never go out twice under two different nonces.
CREATE TABLE withdrawal_broadcasts (
    withdrawal_id uuid        PRIMARY KEY,
    nonce         bigint      NOT NULL,
    raw_tx        text        NOT NULL,
    tx_hash       text        NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);
