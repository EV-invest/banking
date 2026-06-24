-- 0008: persist the resolved TigerBeetle account `flags` on the id-map row.
--
-- TB account flags are immutable on first create, and a logical key's flags are
-- derived (from its `Normal` side) every time `ensure` runs. Persisting the flags
-- chosen at first create lets the adapter recompute the derivation on a later ensure
-- and hard-fail at boot/migration time if it drifted — instead of TB silently
-- rejecting the changed create as a Conflict and parking every transfer forever.
--
-- `flags` is the u16 TB `AccountFlags` bits stored as INTEGER. Backfill existing rows
-- from `code`: the debit-normal accounts (CryptoWallet=10, BankCustody=11,
-- UserShares=60) get `CreditsMustNotExceedDebits` (4); every other (credit-normal)
-- claim gets `DebitsMustNotExceedCredits` (2). This mirrors `account_flags(normal)`
-- in `infrastructure/ledger.rs` (the single source of truth at write time).
ALTER TABLE tb_accounts ADD COLUMN flags INTEGER;
UPDATE tb_accounts SET flags = CASE WHEN code IN (10, 11, 60) THEN 4 ELSE 2 END WHERE flags IS NULL;
ALTER TABLE tb_accounts ALTER COLUMN flags SET NOT NULL;
