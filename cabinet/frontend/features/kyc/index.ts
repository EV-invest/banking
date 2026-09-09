// The slice's whole contract. The client and its result type stay internal: the only
// thing another layer has any business doing with verification is mounting the entry
// point, and a second caller of `startVerification` would be a second place deciding
// what a 503 looks like.
export { StartVerificationRow } from "@/features/kyc/ui/start-verification-row";
