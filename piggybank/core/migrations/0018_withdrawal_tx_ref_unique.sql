-- A settled withdrawal records the on-chain transaction that paid it in `tx_ref`. Each
-- real on-chain send is distinct, so no two withdrawals may ever settle against the SAME
-- transaction — that would be a phantom disbursement (money posted out twice for one
-- transfer), breaking the global sum(custody)==sum(claims) invariant. This partial UNIQUE
-- index is the data-layer backstop for that invariant: the TON confirmation watcher settles
-- by matching an indexer-reported outgoing transfer, and while its matching logic refuses to
-- attribute one transfer to two same-amount withdrawals, this constraint guarantees a second
-- settle onto an already-used tx_ref fails loudly rather than double-paying. NULL tx_refs
-- (unsettled withdrawals) are exempt.
CREATE UNIQUE INDEX withdrawals_tx_ref_unique ON withdrawals (tx_ref) WHERE tx_ref IS NOT NULL;
