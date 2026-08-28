// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// These two functions are a pair, and the bug that prompted the tests was them being
// only half-implemented: `cabinetPath` existed, its inverse did not, and every caller
// comparing `usePathname()` against a plain `/wallet` silently stopped matching when
// `basePath` was removed. Nothing failed loudly — the nav just never highlighted. So
// the round-trip is the property worth pinning down.
import assert from "node:assert/strict";
import test from "node:test";

import { LOCALES } from "@evinvest/i18n";

import { cabinetPath, isNonPagePath, zonePathname } from "./base-path.ts";

test("every locale round-trips back to the path the app reasons in", () => {
  for (const locale of LOCALES) {
    for (const path of ["/", "/wallet", "/admin/revenue", "/invest/alpha-fund"] as const) {
      assert.equal(zonePathname(cabinetPath(locale, path)), path, `${locale} ${path}`);
    }
  }
});

test("the zone prefix comes off, locale and all", () => {
  assert.equal(zonePathname("/en/cabinet/admin/revenue"), "/admin/revenue");
  assert.equal(zonePathname("/ru/cabinet/wallet"), "/wallet");
  // The cabinet root is "/" — not "" — so `active: (p) => p === "/"` still matches.
  assert.equal(zonePathname("/de/cabinet"), "/");
});

test("a path without the locale prefix still loses the zone prefix", () => {
  // `splitLocalePath` reads a missing prefix as the default locale rather than throwing.
  assert.equal(zonePathname("/cabinet/wallet"), "/wallet");
  assert.equal(zonePathname("/cabinet"), "/");
});

test("a path outside the zone is returned unchanged", () => {
  // The conductor's own routes share the origin; mangling them would be worse than
  // leaving a non-match to the caller.
  assert.equal(zonePathname("/en/about"), "/about");
  assert.equal(zonePathname("/"), "/");
});

test("a segment that merely starts with the zone name is not a prefix", () => {
  // "/cabinets" is a different route; slicing by length alone would have eaten it.
  assert.equal(zonePathname("/en/cabinets/list"), "/cabinets/list");
});

// The v0.2.57 outage: dropping `basePath` moved every asset from `/_next/…` to
// `/cabinet/_next/…`, the proxy's anchored exclusions stopped matching, and the
// zone served its own stylesheets and scripts as the login page — unstyled,
// never hydrating. These pin the real production paths, not the stripped ones.
test("real production asset paths are not pages", () => {
  for (const path of [
    "/cabinet/_next/static/chunks/3nl-j504onixu.css",
    "/cabinet/_next/static/chunks/turbopack-32gvlhco9th6x.js",
    "/cabinet/_next/static/media/Inter_Italic-s.p.3ub0m60m9w90g.ttf",
    "/cabinet/_next/image",
    "/cabinet/api/session",
    "/cabinet/mfe/account-chip.js",
    "/cabinet/favicon.ico",
  ]) {
    assert.equal(isNonPagePath(path), true, path);
  }
});

test("the bare forms still hold, for a direct run of the zone", () => {
  for (const path of [
    "/_next/static/chunks/x.css",
    "/_next/image",
    "/api/session",
    "/mfe/account-chip.js",
    "/favicon.ico",
  ]) {
    assert.equal(isNonPagePath(path), true, path);
  }
});

test("pages are pages", () => {
  // Every one of these must keep its locale redirect and its session gate.
  for (const path of [
    "/",
    "/cabinet",
    "/cabinet/wallet",
    "/en/cabinet",
    "/ru/cabinet/wallet",
    "/en/cabinet/login",
    "/de/cabinet/admin/revenue",
  ]) {
    assert.equal(isNonPagePath(path), false, path);
  }
});

test("a page that merely mentions an excluded name is still a page", () => {
  // Guards the lookahead against being loosened into a substring test: these are
  // reader-facing routes and ungating them would expose the (app) shell.
  for (const path of [
    "/en/cabinet/api-keys",
    "/en/cabinet/mfeature",
    "/cabinets/_next/static/x.css",
    "/en/cabinet/_next/static/x.css",
  ]) {
    assert.equal(isNonPagePath(path), false, path);
  }
});
