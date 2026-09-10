// The slice's whole contract: the two places a user can begin verification — a row inside
// the profile's Verification card, and the block a money screen shows instead of a surface
// the hub has closed to them. The client, the hook and their result types stay internal: a
// second caller of `startVerification` would be a second place deciding what a 503 looks
// like, which is exactly what these two presentations exist to share instead.
export { StartVerificationRow } from "@/features/kyc/ui/start-verification-row";
export { VerificationRequired } from "@/features/kyc/ui/verification-required";
