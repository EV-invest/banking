// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { providerUrl } from "./provider-url.ts";

const HOST = "verify.didit.me";
const OK = "https://verify.didit.me/session/abc";

test("an https URL on the allowed host passes through unchanged", () => {
  assert.equal(providerUrl(OK, HOST), OK);
});

test("a non-https scheme is refused whether or not a host is configured", () => {
  for (const allowed of [HOST, undefined]) {
    assert.equal(providerUrl("javascript:alert(1)", allowed), null);
    assert.equal(providerUrl("data:text/html,<script>alert(1)</script>", allowed), null);
    assert.equal(providerUrl("http://verify.didit.me/session/abc", allowed), null);
  }
});

test("an https URL on another host is refused once a host is configured", () => {
  assert.equal(providerUrl("https://evil.example/session/abc", HOST), null);
  // A suffix match would let `notverify.didit.me` and `verify.didit.me.evil.example` through.
  assert.equal(providerUrl("https://notverify.didit.me/s", HOST), null);
  assert.equal(providerUrl("https://verify.didit.me.evil.example/s", HOST), null);
});

test("the port is part of the host", () => {
  assert.equal(providerUrl("https://verify.didit.me:8443/s", HOST), null);
  assert.equal(providerUrl("https://verify.didit.me:8443/s", "verify.didit.me:8443"), "https://verify.didit.me:8443/s");
});

test("the host comparison ignores case on both sides", () => {
  assert.equal(providerUrl("https://VERIFY.Didit.ME/s", HOST), "https://VERIFY.Didit.ME/s");
  assert.equal(providerUrl(OK, " Verify.Didit.ME "), OK);
});

test("several hosts may be allowed at once", () => {
  assert.equal(providerUrl(OK, "staging.didit.me, verify.didit.me"), OK);
  assert.equal(providerUrl("https://other.didit.me/s", "staging.didit.me, verify.didit.me"), null);
});

test("an unset allowlist degrades to the scheme check, never to a block", () => {
  // The authoritative host check is on the plane (it answers 503 for a foreign host). An
  // unset NEXT_PUBLIC_KYC_PROVIDER_HOST must not black-hole a working verification flow.
  assert.equal(providerUrl(OK, undefined), OK);
  assert.equal(providerUrl(OK, ""), OK);
  assert.equal(providerUrl(OK, " , "), OK);
});

test("a missing or unparseable URL is refused", () => {
  assert.equal(providerUrl(null, HOST), null);
  assert.equal(providerUrl("", HOST), null);
  assert.equal(providerUrl("not a url", HOST), null);
});
