-- 0019: swept_at on deposits — scopes the sweep scan to credited, un-swept deposits.
--
-- The sweeps used to read EVERY derived deposit address's on-chain balance every
-- cycle: O(N) RPC per 30s, which melted a free RPC tier at a few hundred addresses
-- (3674 rate-limit errors in one live run). The hub already knows exactly which
-- addresses can still hold funds — those with a credited deposit that has not been
-- consolidated — so the sweep now scans only them. `swept_at` is stamped when a
-- cycle observes the address drained below the sweep minimum: its consolidation
-- mined, or the deposit was dust that will never be worth moving.

ALTER TABLE deposits ADD COLUMN swept_at TIMESTAMPTZ;
CREATE INDEX deposits_unswept_idx ON deposits (network, party_id) WHERE swept_at IS NULL;
