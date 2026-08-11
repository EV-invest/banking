// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// A self-deleting guard over the ONE temporary uikit mirror the cabinet carries. The kit
// is the source of truth for component classes; mirroring any of them here is a stopgap
// that has to expire on its own, because the failure mode is not a broken build — it is a
// duplicate that quietly outlives its reason and then contradicts the kit.
//
// This one exists because `@evinvest/uikit`'s Drawer shipped with no transition at all
// (EV-invest/lib#96 fixes it upstream). Until that lands on npm, the operations timeline
// passes the enter classes at its own `DrawerContent` call site.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

// Resolved through the package's own exports map, so this tracks whatever version is
// installed rather than a guessed path into node_modules. `import.meta.resolve` rather
// than `createRequire().resolve` because the kit's "." export declares only `types` and
// `import` — CJS resolution finds no condition it can use and throws.
const uikitDist = fileURLToPath(import.meta.resolve("@evinvest/uikit"));

const MIRROR_MARKER = "UIKIT-MIRROR: drawer-animation";

// The mirror has two halves and they expire together: the panel's enter classes at the
// call site, and the scrim's fade in CSS (the kit renders that scrim inside DrawerContent
// with no `data-state`, so nothing at the call site can reach it).
const MIRRORED = [
  { what: "the panel's enter classes", at: new URL("../../views/operations/ui/operations-view.tsx", import.meta.url) },
  { what: "the scrim's fade", at: new URL("../../application/styles/globals.css", import.meta.url) },
];

test("the drawer mirror disappears the moment the uikit ships the animation", () => {
  const kit = readFileSync(uikitDist, "utf8");
  const drawerBase = /var DRAWER_CONTENT_BASE = "([^"]*)"/.exec(kit)?.[1];
  assert.ok(drawerBase, "could not read DRAWER_CONTENT_BASE out of the installed uikit");

  const kitAnimates = drawerBase.includes("data-[state=open]:animate-in");

  for (const { what, at } of MIRRORED) {
    const present = readFileSync(at, "utf8").includes(MIRROR_MARKER);
    if (kitAnimates) {
      assert.equal(
        present,
        false,
        `The published uikit now animates its Drawer, so ${what} is a duplicate that will drift.
Delete the block marked "${MIRROR_MARKER}" from ${at.pathname.split("/").slice(-2).join("/")} — and delete this test once both are gone.`,
      );
    } else {
      // While the kit is still unfixed the mirror must stay, or the mobile detail panel
      // goes back to appearing in a single frame with nobody noticing.
      assert.equal(present, true, `The uikit Drawer still has no animation, so ${what} must keep the "${MIRROR_MARKER}" block.`);
    }
  }
});
