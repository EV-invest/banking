-- Per-network resume point for the on-chain deposit watcher: the last block it has fully
-- scanned for confirmed USDT transfers. The existing `deposits` table (0002) remains the
-- idempotency gate for crediting (unique by tx_ref); this only avoids re-scanning blocks.
CREATE TABLE deposit_scan_cursor (
    network            text        PRIMARY KEY,
    last_scanned_block bigint      NOT NULL,
    updated_at         timestamptz NOT NULL DEFAULT now()
);
