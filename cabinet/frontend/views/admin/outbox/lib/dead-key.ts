// Which parked rows are not the operator's to fix.
//
// A withdrawal parks with the signer's own wire message when the signing key cannot
// be unsealed (the KEK-epoch dead-key class). Nothing the cabinet offers helps there:
// unparking re-drives the row into the same refusal, and only a key rotation by
// engineering clears it — and engineering is already paged by the Grafana alert on
// the same signature. So the table swaps the Unpark button for a hint instead.

// Mirrors `UNSEAL_FAILURE_SIGNATURE` in `piggybank/core/src/infrastructure/telemetry.rs`
// (line 13), which is itself a copy of the signer's message in `piggybank/signer/src/backend.rs`.
// The contract is the error *text*, not a code: the hub folds the signer status into the park
// reason as `signer: <message>`, so there is no field to key on. Duplicating the string is the
// honest shape of that contract — if piggybank ever renames it, this hint simply stops firing
// and the row shows the plain Unpark button again; nothing breaks.
const UNSEAL_FAILURE_SIGNATURE = "could not unseal the signing key";

/** Whether a park reason is the signer's dead-key refusal — the one an operator cannot unpark past. */
export function isDeadKeyPark(reason: string): boolean {
  return reason.includes(UNSEAL_FAILURE_SIGNATURE);
}
