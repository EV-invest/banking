// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// A screen skips its server reads when the render answers the router's fetch of an RSC
// payload, and makes them on a document load. The two are told apart by `Sec-Fetch-Mode`;
// a browser that sends none must count as a document load, because skipping the read is
// the optimisation and making it is the safe side.
import assert from "node:assert/strict";
import test from "node:test";

import { installModuleHooks } from "../../__tests__/react-hook-harness.ts";

installModuleHooks({
  "server-only": new URL("./__tests__/server-only.stub.ts", import.meta.url).href,
  "next/headers": new URL("./__tests__/next-headers.stub.ts", import.meta.url).href,
});

const { request } = await import("./__tests__/next-headers.stub.ts");
const { isClientNavigation } = await import("./navigation.ts");

test("a document load is not a client navigation", async () => {
  request.headers = new Headers({ "sec-fetch-mode": "navigate" });

  assert.equal(await isClientNavigation(), false);
});

test("the router's cors fetch of the RSC payload is a client navigation", async () => {
  request.headers = new Headers({ "sec-fetch-mode": "cors" });

  assert.equal(await isClientNavigation(), true);
});

test("the router's same-origin fetch of the RSC payload is a client navigation", async () => {
  request.headers = new Headers({ "sec-fetch-mode": "same-origin" });

  assert.equal(await isClientNavigation(), true);
});

test("a request without Sec-Fetch-Mode is treated as a document load", async () => {
  request.headers = new Headers();

  assert.equal(await isClientNavigation(), false);
});
