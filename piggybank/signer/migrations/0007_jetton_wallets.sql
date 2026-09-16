-- 0007: jetton_wallets — the USDT jetton wallet each TON wallet's transfers go through,
-- learned on first use and held to from then on.
--
-- A jetton transfer is an internal message TO `our_jetton_wallet` carrying `msg_value` in
-- Toncoin; whatever contract sits at that address receives it. The signer never derives that
-- address (it is a hash of the jetton master's wallet code plus the owner, which the hub
-- resolves through an indexer), so until now a deposit sweep's `our_jetton_wallet` went
-- unchecked and the treasury's was checked only when `SIGNER_TON_TREASURY_JETTON_WALLET`
-- was set — and in production it is not. Trust-on-first-use closes most of that: the first
-- transfer a `(wallet, network)` signs pins the address it named, and every later transfer
-- must name the same one (compared rendering-aware, stored in the raw `0:<hex>` form).
--
-- What remains trusted: exactly one message per wallet — the first — bounded by the fee
-- budget's `msg_value` ceiling (0.1 TON). A compromised hub gets one such message per
-- deposit wallet and one for the treasury, never a stream. Deriving the address from the
-- StateInit instead is the follow-up that removes even that.
--
-- The write is a plain `INSERT … ON CONFLICT DO NOTHING`: two first sweeps racing on one
-- wallet both try to pin, one wins, and the loser re-reads the winner's row and is held to
-- it — no advisory lock needed. Rows are never updated or deleted by the signer; a pin an
-- operator has to change (a jetton master migration) is a manual `DELETE` on this table.
--
-- Irreversible? No: additive, and the previous binary neither reads nor writes it. A `down`
-- would be `DROP TABLE jetton_wallets` — this repo's migrator carries no down files (see
-- 0001-0006), so the rollback path is "roll the image back and leave the table in place".
SET lock_timeout = '3s';

CREATE TABLE jetton_wallets (
    wallet_id     UUID NOT NULL,
    network       TEXT NOT NULL,
    jetton_wallet TEXT NOT NULL,
    learned_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (wallet_id, network)
);
