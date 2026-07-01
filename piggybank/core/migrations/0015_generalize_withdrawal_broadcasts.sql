-- Generalize the on-chain broadcast record from EVM-only to multi-chain (BEP20 + TRC20 + TON).
-- The row is still keyed by the network-agnostic `withdrawal_id`; these columns let the per-rail
-- custody adapters share the table:
--   * `network`    — which rail this broadcast is for. Existing rows are all BEP20. The BEP20
--                    custody's nonce sequence is now scoped `WHERE network = 'bep20'`, so TON's
--                    seqno values (also stored in `nonce`) can't pollute the EVM MAX(nonce).
--   * `nonce`      — now nullable: EVM uses the account nonce, TON stores the wallet `seqno`,
--                    but TRON has no nonce at all (replay is ref-block + expiration + txID).
--   * `expiration` — TRON's tx expiration / TON's `valid_until` (unix). NULL for EVM, which
--                    relies on the nonce instead. The TRON confirmation path needs it to decide
--                    whether an un-mined broadcast is still live or provably dead (safe re-sign).
ALTER TABLE withdrawal_broadcasts
    ADD COLUMN network    text NOT NULL DEFAULT 'bep20',
    ALTER COLUMN nonce DROP NOT NULL,
    ADD COLUMN expiration bigint;
