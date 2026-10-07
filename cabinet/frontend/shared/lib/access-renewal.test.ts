// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// Pins the decisions `proxy.ts` makes before a server render reads the access cookie: when a
// token counts as lapsing, which answers from the identity plane are a verdict and which are
// not, that a "gone" verdict hands the plane's clearing cookies through untouched, that a
// burst of requests costs the plane one call, and that a plane with no answer is not asked
// again on every page for RENEW_BACKOFF_MS.
import assert from "node:assert/strict";
import test, { afterEach, beforeEach, mock } from "node:test";

import { RENEW_BACKOFF_MS, RENEW_WITHIN_S, needsRenewal, renewAccess, resetRenewalsForTests, secondsUntilExpiry, setCookieValue, type RenewalRequest } from "./access-renewal.ts";

const NOW_MS = 1_700_000_000_500;
const NOW_S = 1_700_000_000;

/** An unsigned JWT with this payload — the signature is never read here. */
function jwt(payload: object): string {
  const part = (value: object) => Buffer.from(JSON.stringify(value)).toString("base64url");
  return `${part({ alg: "EdDSA", typ: "JWT" })}.${part(payload)}.signature`;
}

interface Call {
  url: string;
  init: RequestInit;
}

/** A fetch that records each call and answers with whatever `answer` returns. */
function fakeFetch(answer: (init: RequestInit) => Promise<Response> | Response) {
  const calls: Call[] = [];
  const fetchImpl = (async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({ url: String(url), init: init ?? {} });
    return answer(init ?? {});
  }) as typeof fetch;
  return { calls, fetchImpl };
}

function json(body: unknown, init: { status?: number; setCookies?: string[] } = {}): Response {
  const headers: [string, string][] = [["content-type", "application/json"]];
  for (const line of init.setCookies ?? []) headers.push(["set-cookie", line]);
  return new Response(JSON.stringify(body), { status: init.status ?? 200, headers });
}

function request(fetchImpl: typeof fetch, session = "sid-1", extra: Partial<RenewalRequest> = {}): RenewalRequest {
  return {
    authWebUrl: "http://concierge.internal:8080/",
    sessionCookie: { name: "__Host-ev_session", value: session },
    accessCookieName: "__Host-ev_access",
    fetch: fetchImpl,
    ...extra,
  };
}

beforeEach(() => {
  resetRenewalsForTests();
});

afterEach(() => {
  mock.timers.reset();
});

// ── secondsUntilExpiry ──────────────────────────────────────────────────────

test("secondsUntilExpiry reads the remaining life of a valid token", () => {
  assert.equal(secondsUntilExpiry(jwt({ sub: "u1", exp: NOW_S + 90 }), NOW_MS), 90);
});

test("secondsUntilExpiry reports an already lapsed token as negative, not unreadable", () => {
  assert.equal(secondsUntilExpiry(jwt({ exp: NOW_S - 5 }), NOW_MS), -5);
});

test("secondsUntilExpiry returns null for a value that is not a JWT", () => {
  assert.equal(secondsUntilExpiry("not-a-jwt", NOW_MS), null);
  assert.equal(secondsUntilExpiry("", NOW_MS), null);
  assert.equal(secondsUntilExpiry("header.%%%not-base64%%%.sig", NOW_MS), null);
});

test("secondsUntilExpiry returns null for a token cut off mid-payload", () => {
  const whole = jwt({ sub: "u1", exp: NOW_S + 90 });
  const [header, payload] = whole.split(".");
  const truncated = `${header}.${payload.slice(0, 12)}`;

  assert.equal(secondsUntilExpiry(truncated, NOW_MS), null);
});

test("secondsUntilExpiry returns null when the payload carries no numeric exp", () => {
  assert.equal(secondsUntilExpiry(jwt({ sub: "u1" }), NOW_MS), null);
  assert.equal(secondsUntilExpiry(jwt({ sub: "u1", exp: String(NOW_S + 90) }), NOW_MS), null);
  assert.equal(secondsUntilExpiry(`${jwt({}).split(".")[0]}.${Buffer.from("[1,2]").toString("base64url")}.sig`, NOW_MS), null);
});

// ── needsRenewal ────────────────────────────────────────────────────────────

test("needsRenewal asks for a token when the request carries none", () => {
  assert.equal(needsRenewal(undefined, NOW_MS), true);
  assert.equal(needsRenewal("", NOW_MS), true);
});

test("needsRenewal renews a token with 59 s left", () => {
  assert.equal(RENEW_WITHIN_S, 60);
  assert.equal(needsRenewal(jwt({ exp: NOW_S + 59 }), NOW_MS), true);
});

test("needsRenewal leaves a token with exactly 60 s left alone", () => {
  assert.equal(needsRenewal(jwt({ exp: NOW_S + 60 }), NOW_MS), false);
});

test("needsRenewal renews a cookie it cannot read rather than render with it", () => {
  assert.equal(needsRenewal("garbage", NOW_MS), true);
});

// ── setCookieValue ──────────────────────────────────────────────────────────

test("setCookieValue reads the value of a __Host- prefixed cookie by its full name", () => {
  assert.equal(setCookieValue("__Host-ev_access=eyJ.abc.def; Path=/; Secure; HttpOnly; SameSite=Lax", "__Host-ev_access"), "eyJ.abc.def");
});

test("setCookieValue does not match the bare name against a prefixed cookie", () => {
  assert.equal(setCookieValue("__Host-ev_access=eyJ.abc.def; Path=/", "ev_access"), null);
});

test("setCookieValue ignores a line that sets another cookie", () => {
  assert.equal(setCookieValue("ev_csrf=tok; Path=/", "ev_access"), null);
  assert.equal(setCookieValue("ev_access_old=tok; Path=/", "ev_access"), null);
});

test("setCookieValue ignores a line with no name=value pair", () => {
  assert.equal(setCookieValue("ev_access; Path=/", "ev_access"), null);
});

test("setCookieValue reports a clearing line as an empty value", () => {
  assert.equal(setCookieValue("ev_access=; Path=/; Max-Age=0", "ev_access"), "");
});

// ── renewAccess: verdicts ───────────────────────────────────────────────────

test("renewAccess reports alive with the access token the plane re-set", async () => {
  const fresh = jwt({ exp: NOW_S + 300 });
  const setCookies = ["__Host-ev_csrf=c1; Path=/; Secure", `__Host-ev_access=${fresh}; Path=/; Secure; HttpOnly`];
  const { fetchImpl } = fakeFetch(() => json({ authenticated: true, user: { userId: "u1" } }, { setCookies }));

  const renewal = await renewAccess(request(fetchImpl));

  assert.deepEqual(renewal, { kind: "alive", access: fresh, setCookies });
});

test("renewAccess reports alive with no access when the plane re-set none", async () => {
  const { fetchImpl } = fakeFetch(() => json({ authenticated: true }, { setCookies: ["__Host-ev_csrf=c1; Path=/"] }));

  const renewal = await renewAccess(request(fetchImpl));

  assert.deepEqual(renewal, { kind: "alive", access: null, setCookies: ["__Host-ev_csrf=c1; Path=/"] });
});

test("renewAccess does not treat a clearing access line as a token", async () => {
  const { fetchImpl } = fakeFetch(() => json({ authenticated: true }, { setCookies: ["__Host-ev_access=; Path=/; Max-Age=0"] }));

  const renewal = await renewAccess(request(fetchImpl));

  assert.equal(renewal.kind, "alive");
  assert.equal(renewal.kind === "alive" ? renewal.access : "unreachable", null);
});

test("renewAccess reports gone and passes every clearing Set-Cookie through verbatim", async () => {
  const clearing = [
    "__Host-ev_session=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0",
    "__Host-ev_csrf=; Path=/; Secure; SameSite=Lax; Max-Age=0",
    "__Host-ev_access=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0",
  ];
  const { fetchImpl } = fakeFetch(() => json({ authenticated: false }, { setCookies: clearing }));

  const renewal = await renewAccess(request(fetchImpl));

  assert.deepEqual(renewal, { kind: "gone", setCookies: clearing });
});

test("renewAccess has no verdict on a 500", async () => {
  const { fetchImpl } = fakeFetch(() => new Response("session store unavailable", { status: 500 }));

  assert.deepEqual(await renewAccess(request(fetchImpl)), { kind: "unknown" });
});

test("renewAccess has no verdict on a redirect — it is not followed", async () => {
  const { calls, fetchImpl } = fakeFetch(() => new Response(null, { status: 302, headers: { location: "/login" } }));

  assert.deepEqual(await renewAccess(request(fetchImpl)), { kind: "unknown" });
  assert.equal(calls[0]?.init.redirect, "manual");
});

test("renewAccess has no verdict when the request throws", async () => {
  const { fetchImpl } = fakeFetch(() => {
    throw new TypeError("fetch failed");
  });

  assert.deepEqual(await renewAccess(request(fetchImpl)), { kind: "unknown" });
});

test("renewAccess has no verdict when the plane does not answer within the timeout", async () => {
  const { fetchImpl } = fakeFetch(
    (init) =>
      new Promise<Response>((_, reject) => {
        init.signal?.addEventListener("abort", () => reject(init.signal?.reason));
      }),
  );

  assert.deepEqual(await renewAccess(request(fetchImpl, "sid-1", { timeoutMs: 5 })), { kind: "unknown" });
});

test("renewAccess has no verdict on a body that is not JSON", async () => {
  const { fetchImpl } = fakeFetch(() => new Response("<html>gateway</html>", { status: 200, headers: { "set-cookie": "ev_access=leaked; Path=/" } }));

  assert.deepEqual(await renewAccess(request(fetchImpl)), { kind: "unknown" });
});

test("renewAccess has no verdict on a body without a boolean authenticated", async () => {
  const { fetchImpl: empty } = fakeFetch(() => json({ user: { userId: "u1" } }));
  const { fetchImpl: stringly } = fakeFetch(() => json({ authenticated: "false" }));
  const { fetchImpl: nullBody } = fakeFetch(() => json(null));

  assert.deepEqual(await renewAccess(request(empty, "sid-a")), { kind: "unknown" });
  resetRenewalsForTests();
  assert.deepEqual(await renewAccess(request(stringly, "sid-b")), { kind: "unknown" });
  resetRenewalsForTests();
  assert.deepEqual(await renewAccess(request(nullBody, "sid-c")), { kind: "unknown" });
});

// ── renewAccess: what leaves the process ────────────────────────────────────

test("renewAccess sends only the session cookie, to /auth/session, with the id out of the URL", async () => {
  const { calls, fetchImpl } = fakeFetch(() => json({ authenticated: true }));

  await renewAccess(request(fetchImpl, "sid-secret-42"));

  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "http://concierge.internal:8080/auth/session");
  assert.equal(calls[0].url.includes("sid-secret-42"), false, "the session id must never be put in a URL");
  assert.deepEqual(calls[0].init.headers, { accept: "application/json", cookie: "__Host-ev_session=sid-secret-42" });
});

// ── renewAccess: single-flight ──────────────────────────────────────────────

test("two simultaneous renewals of one session share a single request", async () => {
  let answer: (res: Response) => void = () => undefined;
  const { calls, fetchImpl } = fakeFetch(() => new Promise<Response>((resolve) => (answer = resolve)));

  const first = renewAccess(request(fetchImpl));
  const second = renewAccess(request(fetchImpl));
  answer(json({ authenticated: true }, { setCookies: ["__Host-ev_access=tok; Path=/"] }));
  const [a, b] = await Promise.all([first, second]);

  assert.equal(calls.length, 1, "a page and its prefetch must cost the identity plane one call");
  assert.deepEqual(a, { kind: "alive", access: "tok", setCookies: ["__Host-ev_access=tok; Path=/"] });
  assert.equal(b, a);
});

test("renewals of two different sessions are not merged", async () => {
  const { calls, fetchImpl } = fakeFetch(() => json({ authenticated: true }));

  await Promise.all([renewAccess(request(fetchImpl, "sid-a")), renewAccess(request(fetchImpl, "sid-b"))]);

  assert.equal(calls.length, 2);
  assert.deepEqual(calls[0].init.headers, { accept: "application/json", cookie: "__Host-ev_session=sid-a" });
  assert.deepEqual(calls[1].init.headers, { accept: "application/json", cookie: "__Host-ev_session=sid-b" });
});

test("a settled renewal is not reused by the next request", async () => {
  const { calls, fetchImpl } = fakeFetch(() => json({ authenticated: true }));

  await renewAccess(request(fetchImpl));
  await renewAccess(request(fetchImpl));

  assert.equal(calls.length, 2);
});

// ── renewAccess: backoff ────────────────────────────────────────────────────

test("after a call with no verdict, renewals are skipped until RENEW_BACKOFF_MS has passed", async () => {
  mock.timers.enable({ apis: ["Date"], now: NOW_MS });
  const { calls, fetchImpl } = fakeFetch(() => new Response("down", { status: 503 }));

  assert.deepEqual(await renewAccess(request(fetchImpl), NOW_MS), { kind: "unknown" });
  assert.deepEqual(await renewAccess(request(fetchImpl, "sid-other"), NOW_MS + RENEW_BACKOFF_MS - 1), { kind: "unknown" });
  assert.equal(calls.length, 1, "inside the window the plane must not be asked again");

  await renewAccess(request(fetchImpl), NOW_MS + RENEW_BACKOFF_MS);
  assert.equal(calls.length, 2, "once the window is over the plane is asked again");
});

test("a gone verdict arms no backoff", async () => {
  mock.timers.enable({ apis: ["Date"], now: NOW_MS });
  const { calls, fetchImpl } = fakeFetch(() => json({ authenticated: false }));

  await renewAccess(request(fetchImpl), NOW_MS);
  await renewAccess(request(fetchImpl), NOW_MS + 1);

  assert.equal(calls.length, 2);
});

test("resetRenewalsForTests lifts an armed backoff", async () => {
  mock.timers.enable({ apis: ["Date"], now: NOW_MS });
  const { calls, fetchImpl } = fakeFetch(() => new Response("down", { status: 503 }));

  await renewAccess(request(fetchImpl), NOW_MS);
  resetRenewalsForTests();
  await renewAccess(request(fetchImpl), NOW_MS + 1);

  assert.equal(calls.length, 2);
});
