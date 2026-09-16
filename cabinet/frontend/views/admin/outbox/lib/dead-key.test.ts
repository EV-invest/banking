// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The hint replaces the Unpark button, so a false positive hides the one action the
// operator has, and a false negative shows a button that silently re-parks. Both edges
// are pinned here against the reason text the hub actually writes.
import assert from "node:assert/strict";
import test from "node:test";

import { isDeadKeyPark } from "./dead-key.ts";

test("the signer's unseal refusal is recognised under the hub's `signer:` prefix", () => {
  assert.equal(isDeadKeyPark("signer: could not unseal the signing key"), true);
  assert.equal(
    isDeadKeyPark("signer: could not unseal the signing key — PROVABLY DEAD KEY (KEK epoch): funds cannot move"),
    true,
  );
});

test("ordinary park reasons keep their Unpark button", () => {
  assert.equal(isDeadKeyPark("signer: key custodian requires approval for activity 0f6a2b3c"), false);
  assert.equal(isDeadKeyPark("rail: insufficient gas on the treasury address"), false);
  assert.equal(isDeadKeyPark("apply failed: account not found"), false);
});

test("an empty reason is not a dead key", () => {
  assert.equal(isDeadKeyPark(""), false);
});

test("the match is case-sensitive, as the signer's wire message is", () => {
  assert.equal(isDeadKeyPark("signer: Could Not Unseal The Signing Key"), false);
});
