// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// Why these exist: the wrong answer here is not an error but a silent PATCH that
// rewrites an account setting on every device (#347). Each case is a page load that
// really happens; the assertion is which of them are allowed to write.
import assert from "node:assert/strict";
import test from "node:test";

import { decideLocaleSync } from "./locale-sync-policy.ts";

test("regression #347: a prefixed URL never overwrites a stored language", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "vi", guessed: false, cookie: "de" }), {
    kind: "none",
  });
});

test("an account with no language yet is seeded from a deliberate prefixed URL", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "", guessed: false, cookie: "de" }), {
    kind: "store",
    language: "de",
  });
});

test("the unprefixed locale is never taken as a choice", () => {
  assert.deepEqual(decideLocaleSync({ locale: "en", stored: "", guessed: false, cookie: "en" }), {
    kind: "none",
  });
});

test("a guessed URL yields to the stored language", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "vi", guessed: true, cookie: "de" }), {
    kind: "adopt-stored",
    to: "vi",
  });
});

test("a guessed URL stores nothing when the account has no language", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "", guessed: true, cookie: "de" }), {
    kind: "none",
  });
});

test("URL and stored language already agree", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "de", guessed: false, cookie: "de" }), {
    kind: "none",
  });
});

test("a stored language the UI cannot express is left alone", () => {
  assert.deepEqual(
    decideLocaleSync({ locale: "de", stored: "en-GB", guessed: false, cookie: "de" }),
    { kind: "none" },
  );
});

test("the settings switcher's cookie-then-navigate window is not raced", () => {
  assert.deepEqual(decideLocaleSync({ locale: "de", stored: "", guessed: false, cookie: "fr" }), {
    kind: "none",
  });
});
