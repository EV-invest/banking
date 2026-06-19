-- 0003: user wallets — the withdrawal saga + per-user deposit addresses.
--
-- Extends the money plane (0002) with the user-facing withdraw/deposit surface.
-- TigerBeetle stays the authoritative store of amounts (the data plane); Postgres
-- holds ONLY ids, state, the destination/derived address, and the fee — never a
-- second copy of a balance. USDT amounts are exact integer counts of canonical
-- 18-decimal base units, stored as TEXT and parsed to u128 in the adapter.

-- (1) withdrawals: the Withdrawal aggregate's write store AND read projection. The
-- two-phase saga's state (pending → completed | failed) lives here; the reserved /
-- posted / voided transfers live in TigerBeetle. `amount` is the gross debited from
-- the user, `fee` the retained network fee (net = amount − fee leaves on-chain).
-- `tx_ref` is the on-chain reference, set on settle. Single-transition updates are
-- guarded by the row lock + the aggregate rule in the adapter.
CREATE TABLE withdrawals (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL,
    network    TEXT NOT NULL,
    address    TEXT NOT NULL,
    amount     TEXT NOT NULL CHECK (amount ~ '^[1-9][0-9]*$'),
    fee        TEXT NOT NULL CHECK (fee ~ '^[0-9]+$'),
    state      TEXT NOT NULL,
    tx_ref     TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX withdrawals_user_idx ON withdrawals (user_id);

-- (2) user_deposit_addresses: a user's stable per-network address to receive USDT at.
-- On account-model chains (BEP20/TRC20) a per-user address is the only way to
-- attribute an incoming transfer (a USDT transfer carries no memo). Address metadata
-- only — the credited balance is authoritative in the TigerBeetle user claim.
-- (Stub-derived and cached here until the real HD-derivation service lands.)
CREATE TABLE user_deposit_addresses (
    user_id    UUID NOT NULL,
    network    TEXT NOT NULL,
    address    TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, network)
);
