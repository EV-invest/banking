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
// presentations exist to share instead. Two reads leave: `useKycStatus` (the checklist's
// "what may this screen offer?") and `useKycGate` (the money screens' half of the same
// read — whether a surface stands closed; the subscribe form asks the wallet's question).
// One export beats a fourth view reaching into `model/`. The shell's standing status —
// a chip in the rail and a dot on the mobile tab (#395) — is a presentation of the same
// read, so it lives here too rather than the rail composing a tier and a case for itself.
export { useKycStatus, type KycGate } from "@/features/kyc/model/use-kyc-status";
export { useKycGate } from "@/features/kyc/model/use-kyc-gate";
// `Step` is the one presentational exception: the login page tells a newcomer the same
// three-step story before they sign in (#391), and two copies of the row would drift.
export { Step } from "@/features/kyc/ui/step";
export { StartVerificationRow } from "@/features/kyc/ui/start-verification-row";
export { KycStatusChip, KycStatusDot } from "@/features/kyc/ui/status-chip";
export { VerificationRequired } from "@/features/kyc/ui/verification-required";
// The trust line beside a money input (#385): what verification protects, said next to the
// figure it protects rather than in a footer the cabinet does not have.
export { CustodyNote } from "@/features/kyc/ui/custody-note";
export { VerifyButton } from "@/features/kyc/ui/verify-button";
