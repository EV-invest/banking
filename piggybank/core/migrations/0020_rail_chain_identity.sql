-- 0020: pin each EVM rail's persisted state to the chain it was first bound to, so
-- re-pointing an endpoint under a live rail refuses to boot instead of silently corrupting.
--
-- Per-rail state is keyed by the flat network string ('bep20', 'polygon'), which names a
-- RAIL, not a chain. Swapping POLYGON_RPC_URL from Amoy to mainnet therefore inherits the
-- Amoy state under the 'polygon' key: deposit_scan_cursor resumes at an Amoy block height
-- (~10^5 eth_getLogs before the first credit), and withdrawal_broadcasts seeds the treasury
-- nonce from the Amoy high-water mark (custody.rs `chain.max(local_next)` — both chains share
-- the treasury ADDRESS, since the signer keys by (user, network), not by chain), so the first
-- mainnet withdrawal signs into a nonce gap, never mines, and every later one queues behind it.
--
-- Comparing the node's eth_chainId against the config cannot see this: on a correct flip the
-- endpoint and the config agree. Only state that carries the chain it belongs to can — and a
-- config-vs-DB check needs no RPC, so an unreachable node can never turn a transient chain
-- outage into a failed boot for the whole hub.
--
-- Trust-on-first-use: a rail's first boot records its chain id; every later boot must agree.
-- No rows are seeded — a literal ('bep20', 56) would false-positive every dev database on BSC
-- testnet (chain 97), and there is nothing to protect retroactively: Polygon has never run
-- live, and BEP20's live binding is re-asserted from its own config on the next deploy. TON and
-- TRON are exempt: their deposit cursors are wall-clock watermarks, chain-independent by
-- construction.
CREATE TABLE rail_chain_identity (
    network  text        PRIMARY KEY,
    chain_id bigint      NOT NULL,
    bound_at timestamptz NOT NULL DEFAULT now()
);
