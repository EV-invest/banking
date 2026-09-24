-- A broadcast row can now name a send this hub never holds the bytes of: a mined EVM transfer
-- adopted after a Postgres restore took its original row back. `raw_tx` NULL = adopted,
-- nothing to re-send; `tx_hash` is what the confirmation watcher settles on either way.
ALTER TABLE withdrawal_broadcasts ALTER COLUMN raw_tx DROP NOT NULL;
