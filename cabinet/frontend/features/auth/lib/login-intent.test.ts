// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { LOGIN_INTENTS, loginIntent, loginPageHref } from "./login-intent.ts";

test("only `signup` opens the newcomer state; everything else is a returning reader", () => {
  assert.equal(loginIntent("signup"), "signup");
  assert.equal(loginIntent("login"), "login");
  assert.equal(loginIntent(undefined), "login");
  assert.equal(loginIntent(""), "login");
  assert.equal(loginIntent("SIGNUP"), "login");
  assert.equal(loginIntent("register"), "login");
  for (const intent of LOGIN_INTENTS) assert.equal(loginIntent(intent), intent);
});

test("the state switch keeps the reader's returnTo and drops the default intent", () => {
  assert.equal(loginPageHref("login", undefined), "/login");
  assert.equal(loginPageHref("signup", undefined), "/login?intent=signup");
  assert.equal(loginPageHref("signup", "/wallet"), "/login?intent=signup&returnTo=%2Fwallet");
  assert.equal(loginPageHref("login", "/invest/x?y=1"), "/login?returnTo=%2Finvest%2Fx%3Fy%3D1");
});

test("a returnTo that could leave the origin does not survive the switch", () => {
  assert.equal(loginPageHref("signup", "//evil.example"), "/login?intent=signup");
  assert.equal(loginPageHref("login", "https://evil.example/"), "/login");
});
