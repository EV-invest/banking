// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// Covers the imperative half of the cache — the half that decides whether a screen paints
// from memory or from the network. The React hook is exercised by the app; what is worth
// pinning here is the policy: a fresh value is never re-fetched, a stale one is, a failed
// read never destroys the value already held, and a mutation's tags reach every key.
import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";

import { clearResources, defineResource, resetResourcesForTests, revalidateTag } from "./resource.ts";

beforeEach(() => {
  resetResourcesForTests();
});

/** A fetcher that counts its calls and can be made to fail. */
function counted<T>(value: () => T) {
  const state = { calls: 0, fail: null as Error | null };
  const fetch = async () => {
    state.calls += 1;
    if (state.fail) throw state.fail;
    return value();
  };
  return { state, fetch };
}

test("a fresh value is served without touching the network", async () => {
  const { state, fetch } = counted(() => "100.00");
  const balance = defineResource({ name: "t.balance", fetch, revalidate: 60 });

  assert.equal(await balance.read(), "100.00");
  assert.equal(await balance.read(), "100.00");
  assert.equal(state.calls, 1, "the second read must come from cache");
});

test("concurrent readers share one request", async () => {
  const { state, fetch } = counted(() => "100.00");
  const balance = defineResource({ name: "t.balance", fetch });

  await Promise.all([balance.read(), balance.read(), balance.read()]);
  assert.equal(state.calls, 1, "three simultaneous screens must not make three requests");
});

test("a value past its window is re-read", async () => {
  const { state, fetch } = counted(() => "100.00");
  // 0s: every read revalidates. The cached value is still served first — that is the
  // difference between a background refresh and a skeleton.
  const balance = defineResource({ name: "t.balance", fetch, revalidate: 0 });

  await balance.read();
  await balance.read();
  assert.equal(state.calls, 2);
});

test("arguments key separate entries", async () => {
  const seen: string[] = [];
  const nav = defineResource({
    name: "t.nav",
    fetch: async (service: string) => {
      seen.push(service);
      return `nav:${service}`;
    },
    key: (service) => service,
    revalidate: 60,
  });

  assert.equal(await nav.read("alpha"), "nav:alpha");
  assert.equal(await nav.read("beta"), "nav:beta");
  assert.equal(await nav.read("alpha"), "nav:alpha");
  assert.deepEqual(seen, ["alpha", "beta"], "the second alpha read must come from cache");
});

test("a failed refresh keeps the value already on screen", async () => {
  const { state, fetch } = counted(() => "100.00");
  const balance = defineResource({ name: "t.balance", fetch, revalidate: 0 });

  await balance.read();
  state.fail = new Error("gateway timeout");
  await balance.read().catch(() => undefined);

  // The regression this guards: blanking a balance because one poll timed out.
  assert.equal(balance.peek(), "100.00");
});

test("a tag reaches every key carrying it", async () => {
  const { state, fetch } = counted(() => "100.00");
  const balance = defineResource({ name: "t.balance", fetch, revalidate: 600, tags: ["wallet"] });

  await balance.read();
  assert.equal(state.calls, 1);

  // A withdrawal moved the balance. The window says "fresh"; the tag overrules it.
  revalidateTag("wallet");
  await balance.read();
  assert.equal(state.calls, 2);
});

test("a tag nothing carries changes nothing", async () => {
  const { state, fetch } = counted(() => "100.00");
  const balance = defineResource({ name: "t.balance", fetch, revalidate: 600, tags: ["wallet"] });

  await balance.read();
  revalidateTag("notifications");
  await balance.read();
  assert.equal(state.calls, 1);
});

test("invalidateAll reaches every key of one resource, and no others", async () => {
  const { state: navState, fetch: navFetch } = counted(() => "1.00");
  const nav = defineResource({ name: "t.nav", fetch: (service: string) => navFetch().then((v) => `${service}:${v}`), key: (s) => s, revalidate: 600 });
  const { state: otherState, fetch: otherFetch } = counted(() => "x");
  const other = defineResource({ name: "t.other", fetch: otherFetch, revalidate: 600 });

  await nav.read("alpha");
  await nav.read("beta");
  await other.read();
  assert.equal(navState.calls, 2);

  nav.invalidateAll();
  await nav.read("alpha");
  await nav.read("beta");
  await other.read();
  assert.equal(navState.calls, 4);
  assert.equal(otherState.calls, 1, "a sibling resource must be untouched");
});

test("publish writes through without a request", async () => {
  const { state, fetch } = counted(() => "100.00");
  const balance = defineResource({ name: "t.balance", fetch, revalidate: 600 });

  // A PATCH answers with the new state; asking the server what it just told us is waste.
  balance.publish("250.00");
  assert.equal(await balance.read(), "250.00");
  assert.equal(state.calls, 0);
});

test("clearResources drops every account-scoped value", async () => {
  const { fetch } = counted(() => "100.00");
  const balance = defineResource({ name: "t.balance", fetch, revalidate: 600 });

  await balance.read();
  assert.equal(balance.peek(), "100.00");

  // Sign-out. The next account in this tab must not see the previous one's figures.
  clearResources();
  assert.equal(balance.peek(), undefined);
});

test("a disabled read is never issued", async () => {
  const { state, fetch } = counted(() => "nav");
  const nav = defineResource({
    name: "t.nav",
    fetch: (service: string) => fetch().then((v) => `${service}:${v}`),
    key: (service) => service,
    enabled: (service) => service.trim().length > 0,
  });

  // The BFF 400s a NAV request with no fund; a screen that hasn't picked one asks nothing.
  nav.prefetch("");
  assert.equal(state.calls, 0);
  nav.prefetch("alpha");
  await nav.read("alpha");
  assert.equal(state.calls, 1);
});

test("a read that has never succeeded rejects rather than resolving empty", async () => {
  const { state, fetch } = counted(() => "100.00");
  state.fail = new Error("gateway timeout");
  const balance = defineResource({ name: "t.balance", fetch });

  await assert.rejects(() => balance.read(), /gateway timeout/);
  assert.equal(balance.peek(), undefined);
});

// ── a refusal is a verdict, not a blip ────────────────────────────────────────

/** The shape the transport throws for a 403 (`./api-client.ts` — `RequestError`). */
class Forbidden extends Error {
  readonly status = 403;
  constructor() {
    super("You don't have access to this.");
  }
}

test("a refused read is not repeated by the clock", async () => {
  const { state, fetch } = counted(() => "roster");
  state.fail = new Forbidden();
  // 0s, so every automatic trigger would otherwise find it stale and fire again.
  const owners = defineResource({ name: "t.owners", fetch, revalidate: 0 });

  await assert.rejects(() => owners.read(), /access/);
  await assert.rejects(() => owners.read(), /access/);
  owners.prefetch();
  assert.equal(state.calls, 1, "an operator's 403 must not be collected once per poll");
});

test("a refusal suppresses the repeat, never the error", async () => {
  const { state, fetch } = counted(() => "roster");
  state.fail = new Forbidden();
  const owners = defineResource({ name: "t.owners", fetch, revalidate: 0 });

  // The screens branch on this — the consilium renders its "you cannot see this room"
  // state off exactly this error. Swallowing it would trade a noisy console for a blank
  // page that never explains itself.
  await assert.rejects(() => owners.read(), /access/);
  assert.equal(state.calls, 1);
});

test("only a refusal latches — a timeout is still retried", async () => {
  const { state, fetch } = counted(() => "roster");
  state.fail = new Error("gateway timeout");
  const owners = defineResource({ name: "t.owners", fetch, revalidate: 0 });

  await assert.rejects(() => owners.read(), /timeout/);
  await assert.rejects(() => owners.read(), /timeout/);
  assert.equal(state.calls, 2, "a transient failure is exactly what the retry loop is for");
});

test("a tag invalidation lifts a refusal — a seat may have just been granted", async () => {
  const { state, fetch } = counted(() => "roster");
  state.fail = new Forbidden();
  const owners = defineResource({ name: "t.owners", fetch, revalidate: 0, tags: ["owners"] });

  await assert.rejects(() => owners.read(), /access/);
  assert.equal(state.calls, 1);

  // A mutation naming the tag is new information, unlike a timer firing again.
  revalidateTag("owners");
  state.fail = null;
  assert.equal(await owners.read(), "roster");
  assert.equal(state.calls, 2);
});

test("sign-out lifts a refusal — the next account is a different caller", async () => {
  const { state, fetch } = counted(() => "roster");
  state.fail = new Forbidden();
  const owners = defineResource({ name: "t.owners", fetch, revalidate: 0 });

  await assert.rejects(() => owners.read(), /access/);
  clearResources();
  state.fail = null;
  assert.equal(await owners.read(), "roster");
  assert.equal(state.calls, 2);
});
