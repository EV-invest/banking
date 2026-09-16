// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// Pins the two halves of the rule in `cabinet-link.tsx`: cabinet links do not prefetch
// automatically BECAUSE no route has a loading boundary for a prefetch to carry. The
// halves are checked together so they cannot drift apart silently — a `loading.tsx`
// added without revisiting the default would leave real skeletons unprefetched, and a
// prefetch re-enabled without one would bring back two RSC documents per link on every
// page load (banking#349).
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, relative } from "node:path";
import test from "node:test";

const FRONTEND_ROOT = fileURLToPath(new URL("../../", import.meta.url));
const APP_ROOT = join(FRONTEND_ROOT, "app");
const LINK_SOURCE = fileURLToPath(new URL("./cabinet-link.tsx", import.meta.url));

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) walk(path, out);
    else out.push(path);
  }
  return out;
}

test("no cabinet route has a loading boundary — the premise behind prefetch={false}", () => {
  const boundaries = walk(APP_ROOT)
    .filter((path) => /^loading\.(tsx|ts|jsx|js)$/.test(path.slice(path.lastIndexOf("/") + 1)))
    .map((path) => relative(FRONTEND_ROOT, path));
  assert.deepEqual(
    boundaries,
    [],
    `A loading boundary changes what a prefetch is worth: with one, the viewport prefetch ` +
      `carries the layouts and the skeleton, so the click paints before the page arrives. ` +
      `Revisit the prefetch default in shared/ui/cabinet-link.tsx before keeping: ${boundaries.join(", ")}`,
  );
});

test("cabinet links opt out of automatic prefetching unless the caller says otherwise", () => {
  const source = readFileSync(LINK_SOURCE, "utf8");
  // The default sits immediately before the caller's spread, so an explicit `prefetch`
  // from a caller with a reason still wins.
  assert.ok(
    source.includes("prefetch={false} {...props}"),
    "cabinet-link.tsx must render <NextLink … prefetch={false} {...props} /> (banking#349)",
  );
});
