// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// While its reads are out, the checklist tells the skeleton what it is about to become:
// `complete` on money reads that already show funded-and-invested, `path` on money reads
// that do not, `unknown` while either money read is still out (the skeleton then trusts the
// shape cookie). Once settled, the block writes its shape into that cookie for the next
// server render — and only then: a loading or failed read says nothing about the account.
//
// The entity resources are stand-ins whose snapshots each test sets; the checklist logic,
// its memory and the shape cookie are the real modules.
import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";

import { installModuleHooks, renderHook } from "../../../shared/__tests__/react-hook-harness.ts";

const RESOURCES = new URL("./__tests__/resources.stub.ts", import.meta.url).href;
installModuleHooks({
  "@/shared/lib/resource": new URL("./__tests__/resource.stub.ts", import.meta.url).href,
  "@/entities/fund/model/fund-resource": RESOURCES,
  "@/entities/user/model/profile-resource": RESOURCES,
  "@/entities/wallet/model/wallet-resource": RESOURCES,
});

const { snapshots } = await import("./__tests__/resource.stub.ts");
const { positionsResource, profileResource, walletResource } = await import("./__tests__/resources.stub.ts");
const { useChecklist } = await import("./use-checklist.ts");

type Verification = Parameters<typeof useChecklist>[0];

const KYC_LOADING: Verification = { level: 0, runningCase: null, known: false, loading: true };
const KYC_VERIFIED: Verification = { level: 1, runningCase: null, known: true, loading: false };

/** Every cookie the hook wrote, in order. */
let cookieWrites: string[] = [];

beforeEach(() => {
  snapshots.clear();
  snapshots.set(profileResource, { data: { email: "a@b.c" }, isLoading: false });
  cookieWrites = [];
  (globalThis as { document?: unknown }).document = {
    set cookie(value: string) {
      cookieWrites.push(value);
    },
  };
});

const loaded = (data: unknown) => ({ data, isLoading: false });
const loading = { data: undefined, isLoading: true };
const failed = { data: undefined, isLoading: false };

test("with the wallet still out the skeleton cannot know the shape", async () => {
  snapshots.set(walletResource, loading);
  snapshots.set(positionsResource, loaded({ positions: [{ service: "arb" }] }));

  const hook = await renderHook(useChecklist, KYC_LOADING);

  assert.deepEqual(hook.current, { loading: true, checklist: null, expect: "unknown" });
});

test("with the positions still out the skeleton cannot know the shape", async () => {
  snapshots.set(walletResource, loaded({ balance: { total: "250.00" } }));
  snapshots.set(positionsResource, loading);

  const hook = await renderHook(useChecklist, KYC_LOADING);

  assert.deepEqual(hook.current, { loading: true, checklist: null, expect: "unknown" });
});

test("money in and units held shape the skeleton as finished while verification is out", async () => {
  snapshots.set(walletResource, loaded({ balance: { total: "250.00" } }));
  snapshots.set(positionsResource, loaded({ positions: [{ service: "arb" }] }));

  const hook = await renderHook(useChecklist, KYC_LOADING);

  assert.deepEqual(hook.current, { loading: true, checklist: null, expect: "complete" });
});

test("an empty balance shapes the skeleton as the path even with units held", async () => {
  snapshots.set(walletResource, loaded({ balance: { total: "0" } }));
  snapshots.set(positionsResource, loaded({ positions: [{ service: "arb" }] }));

  const hook = await renderHook(useChecklist, KYC_LOADING);

  assert.deepEqual(hook.current, { loading: true, checklist: null, expect: "path" });
});

test("money in but nothing held shapes the skeleton as the path", async () => {
  snapshots.set(walletResource, loaded({ balance: { total: "250.00" } }));
  snapshots.set(positionsResource, loaded({ positions: [] }));

  const hook = await renderHook(useChecklist, KYC_LOADING);

  assert.deepEqual(hook.current, { loading: true, checklist: null, expect: "path" });
});

test("a wallet without a balance and a positions body without a list shape the path", async () => {
  snapshots.set(walletResource, loaded({}));
  snapshots.set(positionsResource, loaded({}));

  const hook = await renderHook(useChecklist, KYC_LOADING);

  assert.deepEqual(hook.current, { loading: true, checklist: null, expect: "path" });
});

test("while anything is out no shape is written for the server", async () => {
  snapshots.set(walletResource, loaded({ balance: { total: "250.00" } }));
  snapshots.set(positionsResource, loaded({ positions: [{ service: "arb" }] }));

  await renderHook(useChecklist, KYC_LOADING);

  assert.deepEqual(cookieWrites, []);
});

test("a finished path in a browser that never saw it open is remembered as no block", async () => {
  snapshots.set(walletResource, loaded({ balance: { total: "250.00" } }));
  snapshots.set(positionsResource, loaded({ positions: [{ service: "arb" }] }));

  await renderHook(useChecklist, KYC_VERIFIED);

  assert.deepEqual(cookieWrites, ["ev_checklist_shape=none; Path=/; Max-Age=31536000; SameSite=Lax"]);
});

test("an open step is remembered as the path card", async () => {
  snapshots.set(walletResource, loaded({ balance: { total: "0" } }));
  snapshots.set(positionsResource, loaded({ positions: [] }));

  await renderHook(useChecklist, KYC_VERIFIED);

  assert.deepEqual(cookieWrites, ["ev_checklist_shape=path; Path=/; Max-Age=31536000; SameSite=Lax"]);
});

test("a failed money read leaves the last remembered shape standing", async () => {
  snapshots.set(walletResource, failed);
  snapshots.set(positionsResource, loaded({ positions: [{ service: "arb" }] }));

  const hook = await renderHook(useChecklist, KYC_VERIFIED);

  assert.deepEqual(hook.current, { loading: false, checklist: null });
  assert.deepEqual(cookieWrites, []);
});
