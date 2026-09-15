// The slice's whole contract: the three places a user can be offered verification — a row in
// the profile's identity card, a banner on the home screen, and the block a money screen
// shows instead of a surface the hub has closed to them.
//
// Everything else stays internal, `VerificationDialog` included. It is the one explanation of
// verification in the cabinet, but the three presentations above are the only things that
// open it, and they are all inside this slice: exporting it named a consumer outside that
// never materialised (#215 was answered by giving the wallet `VerificationRequired`, not the
// dialog), and a public export with no caller is a contract kept for nobody. The client, the
// hooks and their result types stay internal for the older reason — a second caller of
// `startVerification` would be a second place deciding what a 503 looks like, which is
// exactly what these presentations exist to share instead.
export { StartVerificationRow } from "@/features/kyc/ui/start-verification-row";
export { VerificationBanner } from "@/features/kyc/ui/verification-banner";
export { VerificationRequired } from "@/features/kyc/ui/verification-required";
