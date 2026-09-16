-- 0006: native_spend learns an `asset` — the ledger now also windows the USDT a treasury
-- payout moves, not only the native coin a signature commits to.
--
-- The per-transfer USDT cap (`SIGNER_MAX_TRANSFER_USDT`) bounds ONE treasury payout; nothing
-- bounded how many of them a compromised hub asks for in an hour, the same hole 0005 closed
-- for the native coin. Rather than a second table with the same shape, lock and sweep, the
-- one ledger keys its window on `(wallet_id, network, asset)`: `native` rows are what 0005
-- recorded, `usdt` rows are a treasury payout's amount in the network's on-chain base units,
-- refused when the treasury's hour on that rail would exceed
-- `SIGNER_MAX_TREASURY_USDT_PER_HOUR`.
--
-- Expand-only and safe under a mixed rollout. The column defaults to `native`, so the
-- previous binary's INSERT (which names no asset) still lands as a native row; its window SUM
-- names no asset either, so during the rollout it would count any fresh `usdt` rows against
-- the native window — an over-refusal on the treasury for at most one window, never a hole.
-- On Postgres 11+ ADD COLUMN with a constant DEFAULT is metadata-only (no rewrite), and the
-- table holds one hour of rows per wallet in any case.
--
-- The 0005 index is replaced, not kept beside the new one: every query now names the asset,
-- and the new index's `(wallet_id, network)` prefix serves anything the old one served.
--
-- Irreversible? No: a `down` would be DROP INDEX / CREATE INDEX the 0005 one back / DROP
-- CONSTRAINT / DROP COLUMN — this repo's migrator carries no down files (see 0001-0005), so
-- the rollback path is "roll the image back and leave the column in place", which the
-- previous binary tolerates as described above.
SET lock_timeout = '3s';

ALTER TABLE native_spend
    ADD COLUMN asset TEXT NOT NULL DEFAULT 'native';

ALTER TABLE native_spend
    ADD CONSTRAINT native_spend_asset_known CHECK (asset IN ('native', 'usdt'));

CREATE INDEX native_spend_wallet_network_asset_signed_at ON native_spend (wallet_id, network, asset, signed_at);

DROP INDEX native_spend_wallet_network_signed_at;
