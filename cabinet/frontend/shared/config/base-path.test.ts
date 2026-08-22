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

import { cabinetPath, zonePathname } from "./base-path.ts";

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
