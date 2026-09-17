// The slice's whole contract: the places a user can be offered verification — a row in the
// profile's identity card, the block a money screen shows instead of a surface the hub has
// closed to them, and a bare button for a frame that belongs to someone else (the first-run
// checklist on Home, #382) — plus the one reading of "what may this screen offer?" that
// checklist needs to decide whether to show the button at all.
//
// Everything else stays internal, `VerificationDialog` included. It is the one explanation of
// verification in the cabinet, but the presentations above are the only things that open it,
// and they are all inside this slice: exporting it named a consumer outside that never
// materialised (#215 was answered by giving the wallet `VerificationRequired`, not the
// dialog), and a public export with no caller is a contract kept for nobody. The client and
// the start hook stay internal for the older reason — a second caller of `startVerification`
// would be a second place deciding what a 503 looks like, which is exactly what these
// presentations exist to share instead. `useKycStatus` is a read, not a start, and is the
// one hook that leaves.
export { useKycStatus, type KycGate } from "@/features/kyc/model/use-kyc-status";
export { StartVerificationRow } from "@/features/kyc/ui/start-verification-row";
export { VerificationRequired } from "@/features/kyc/ui/verification-required";
export { VerifyButton } from "@/features/kyc/ui/verify-button";
