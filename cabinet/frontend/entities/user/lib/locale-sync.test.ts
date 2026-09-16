// Run with `npm run test`. Each case is a page load that really happens; the assertion
// is that none of them may write to the account (#347) and which one re-enters the page.
import assert from "node:assert/strict";
import test from "node:test";

import { decideLocaleSync } from "./locale-sync.ts";

test("regression #347: a prefixed URL never touches a stored language", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "vi", marker: null }), {
    kind: "none",
  });
});

test("a guessed URL yields to the stored language", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "vi", marker: "de" }), {
    kind: "adopt-stored",
    to: "vi",
  });
});

test("a guessed URL stores nothing when the account has no language", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "", marker: "de" }), {
    kind: "none",
  });
});

test("a guess that matches the stored language is left as it is", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "de", marker: "de" }), {
    kind: "none",
  });
});

test("URL and stored language already agree", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "de", marker: null }), {
    kind: "none",
  });
});

test("a stored language the UI cannot express is left alone, even under a guess", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "en-GB", marker: "de" }), {
    kind: "none",
  });
});

test("a stale marker for another locale is inert", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "vi", marker: "ru" }), {
    kind: "none",
  });
});
