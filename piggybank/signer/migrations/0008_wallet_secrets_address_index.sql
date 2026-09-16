-- 0008: an index for the gas top-up gate's lookup by address.
--
-- `WalletSecrets::find_active_by_address` answers "does this signer hold a key for the
-- top-up's destination on this network?" — one query per gas drip, against the ACTIVE rows
-- only. Until now it was a sequential scan of `wallet_secrets`, fine at dozens of rows and
-- a per-drip cost that grows with every deposit address provisioned.
--
-- The expression is `lower(address)`, not `address`: the store holds EIP-55 on the EVM
-- rails (the signer's own `render_address` output), the raw `0:<hex>` form on TON and
-- Base58Check on Tron, while a top-up may name the address in any rendering the network
-- accepts. Lowercasing both sides makes the EVM lookup an equality, costs nothing on TON
-- (already lowercase hex) and on Tron narrows to case-variants the caller then settles
-- rendering-aware. Partial on `superseded_at IS NULL`, like the 0002 uniqueness index: a
-- rotated-away address is not held.
--
-- Plain CREATE INDEX, not CONCURRENTLY: the table is small (one row per provisioned
-- address) and the migration runs inside the migrator's transaction at boot, where
-- CONCURRENTLY is not allowed; `lock_timeout` bounds the wait for the share lock.
--
-- Irreversible? No: a `down` would be `DROP INDEX wallet_secrets_active_network_lower_address`
-- — this repo's migrator carries no down files (see 0001-0007), so the rollback path is
-- "roll the image back and leave the index in place", which no binary minds.
SET lock_timeout = '3s';

CREATE INDEX wallet_secrets_active_network_lower_address
    ON wallet_secrets (network, lower(address))
    WHERE superseded_at IS NULL;
