// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// Pins the proxy's session decision (`proxy.ts` itself imports `next/server` and cannot be
// loaded here): a renewed token rides on the forwarded request, a session the identity plane
// calls gone is bounced to /login with the plane's clearing cookies, a verdict-less renewal
// changes nothing, and returnTo stays zone-relative.
import assert from "node:assert/strict";
import test from "node:test";

import type { Renewal } from "./access-renewal.ts";
import { decideSession, type SessionGateInput } from "./session-gate.ts";

const BARE = { session: "ev_session", access: "ev_access" };

const CLEARING = [
  "ev_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
  "ev_csrf=; Path=/; SameSite=Lax; Max-Age=0",
  "ev_access=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
];

function input(overrides: Partial<SessionGateInput>): SessionGateInput {
  return { pathname: "/en/cabinet/wallet", search: "", locale: "en", hasSession: true, renewal: null, cookieNames: BARE, ...overrides };
}

test("an alive renewal puts the new access token on the forwarded request and renders", () => {
  const renewal: Renewal = { kind: "alive", access: "new.jwt.sig", setCookies: ["ev_access=new.jwt.sig; Path=/; HttpOnly"] };

  assert.deepEqual(decideSession(input({ renewal })), {
    redirect: null,
    requestCookies: { ev_access: "new.jwt.sig" },
    deleteCookies: [],
    setCookies: ["ev_access=new.jwt.sig; Path=/; HttpOnly"],
  });
});

test("an alive renewal without a re-set token leaves the request alone but still forwards Set-Cookie", () => {
  const renewal: Renewal = { kind: "alive", access: null, setCookies: ["ev_csrf=c2; Path=/"] };

  assert.deepEqual(decideSession(input({ renewal })), {
    redirect: null,
    requestCookies: {},
    deleteCookies: [],
    setCookies: ["ev_csrf=c2; Path=/"],
  });
});

test("a gone session on a gated page bounces to /login carrying the clearing cookies", () => {
  const renewal: Renewal = { kind: "gone", setCookies: CLEARING };

  assert.deepEqual(decideSession(input({ renewal })), {
    redirect: { pathname: "/en/cabinet/login", returnTo: "/wallet" },
    requestCookies: {},
    deleteCookies: ["ev_session", "ev_access"],
    setCookies: CLEARING,
  });
});

test("a gone session on /login renders the login page and still clears the cookies", () => {
  const renewal: Renewal = { kind: "gone", setCookies: CLEARING };

  assert.deepEqual(decideSession(input({ pathname: "/en/cabinet/login", renewal })), {
    redirect: null,
    requestCookies: {},
    deleteCookies: ["ev_session", "ev_access"],
    setCookies: CLEARING,
  });
});

test("a renewal with no verdict changes nothing and lets a signed-in request render", () => {
  assert.deepEqual(decideSession(input({ renewal: { kind: "unknown" } })), {
    redirect: null,
    requestCookies: {},
    deleteCookies: [],
    setCookies: [],
  });
});

test("no session on a public page renders it without a redirect", () => {
  const decision = decideSession(input({ pathname: "/en/cabinet/approve/tok-123", hasSession: false }));

  assert.equal(decision.redirect, null);
});

test("no session on a gated page bounces to /login without asking anyone", () => {
  assert.deepEqual(decideSession(input({ hasSession: false })), {
    redirect: { pathname: "/en/cabinet/login", returnTo: "/wallet" },
    requestCookies: {},
    deleteCookies: [],
    setCookies: [],
  });
});

test("a signed-in visitor on /login is sent to the zone root", () => {
  assert.deepEqual(decideSession(input({ pathname: "/de/cabinet/login", locale: "de" })).redirect, { pathname: "/de/cabinet", returnTo: null });
});

test("returnTo keeps the query and stays zone-relative", () => {
  const decision = decideSession(input({ pathname: "/de/cabinet/wallet/withdraw", search: "?network=tron", locale: "de", hasSession: false }));

  assert.deepEqual(decision.redirect, { pathname: "/de/cabinet/login", returnTo: "/wallet/withdraw?network=tron" });
});

test("returnTo is null for the zone root", () => {
  const decision = decideSession(input({ pathname: "/en/cabinet", hasSession: false }));

  assert.deepEqual(decision.redirect, { pathname: "/en/cabinet/login", returnTo: null });
});

test("a path without a locale is bounced to the English login", () => {
  const decision = decideSession(input({ pathname: "/cabinet/wallet", locale: null, hasSession: false }));

  assert.deepEqual(decision.redirect, { pathname: "/en/cabinet/login", returnTo: "/wallet" });
});

test("__Host- cookie names are used as given for the renewed and the deleted cookies", () => {
  const cookieNames = { session: "__Host-ev_session", access: "__Host-ev_access" };

  const alive = decideSession(input({ cookieNames, renewal: { kind: "alive", access: "tok", setCookies: [] } }));
  const gone = decideSession(input({ cookieNames, renewal: { kind: "gone", setCookies: [] } }));

  assert.deepEqual(alive.requestCookies, { "__Host-ev_access": "tok" });
  assert.deepEqual(gone.deleteCookies, ["__Host-ev_session", "__Host-ev_access"]);
});
