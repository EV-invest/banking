-- 0007: gate placeholder deposit addresses so they can never be served as fundable.
--
-- The signer still emits only a PLACEHOLDER address (a structurally-valid string bound
-- to the real public key, but NOT yet its on-chain cryptographic image — real encoding
-- is a deferred feature). Before this migration every cached row was an undifferentiated
-- placeholder, so the wallet view surfaced it as a real deposit destination and the
-- cache made it permanent. `address_kind` tags each row so the read path can fail closed:
-- a 'placeholder' rail is presented as unavailable, never as a fundable address.
--
-- Existing rows default to 'placeholder' precisely because that is what they are; when
-- real derivation lands the signer reports kind 'derived' and the hub backfills these
-- rows in place (recompute keyed on the signer's stored public_key + key_version).
ALTER TABLE user_deposit_addresses
    ADD COLUMN address_kind TEXT NOT NULL DEFAULT 'placeholder'
        CHECK (address_kind IN ('placeholder', 'derived'));
