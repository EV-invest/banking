// The slice's whole contract: the places a user can be offered verification, and the one
// explanation of it they are offered. The client, the hook and their result types stay
// internal — a second caller of `startVerification` would be a second place deciding what a
// 503 looks like, which is exactly what these presentations exist to share instead.
//
// `VerificationDialog` is exported because the wallet opens it too (#215) and a money screen
// may not reach into `features/kyc/ui/...` for it. It is controlled: the caller owns `open`,
// because the trigger is a row button here, a card there, and a link inside a rail elsewhere.
export { StartVerificationRow } from "@/features/kyc/ui/start-verification-row";
export { VerificationBanner } from "@/features/kyc/ui/verification-banner";
export { VerificationDialog } from "@/features/kyc/ui/verification-dialog";
export { VerificationRequired } from "@/features/kyc/ui/verification-required";
