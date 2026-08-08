// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// `application/styles/globals.css` carries a temporary copy of the token fixes made
// upstream in EV-invest/lib#91, so the cabinet does not have to wait on an npm publish.
// A duplicated source of truth rots silently the moment the real one catches up, so the
// removal is enforced here rather than left to a comment: once the installed uikit ships
// the same values, this test fails and names the block to delete.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import test from "node:test";

// Resolved through the package's own exports map, so this tracks whatever version is
// installed rather than a guessed path into node_modules.
const uikitTokens = createRequire(import.meta.url).resolve("@evinvest/uikit/styles/tokens.css");
const override = new URL("../../application/styles/token-override.css", import.meta.url);
const mfeEntry = new URL("../../mfe/account-chip/mfe.css", import.meta.url);

/** Last wins, matching the cascade — the override block sits after the import. */
function readToken(css: string, name: string): string | undefined {
  const hits = [...css.matchAll(new RegExp(`^\\s*--${name}:\\s*([^;]+);`, "gm"))];
  return hits.at(-1)?.[1].trim();
}

// The values the override exists to supply. Keep in step with the block itself.
const OVERRIDDEN = ["card", "popover", "secondary", "muted", "muted-foreground", "primary", "primary-foreground", "destructive", "destructive-foreground", "border"];

test("the temporary token override is still doing something", () => {
  const upstream = readFileSync(uikitTokens, "utf8");
  const local = readFileSync(override, "utf8");

  const redundant = OVERRIDDEN.filter((name) => {
    const ours = readToken(local, name);
    return ours !== undefined && ours === readToken(upstream, name);
  });

  assert.deepEqual(
    redundant,
    [],
    `@evinvest/uikit now ships these tokens itself: ${redundant.join(", ")}. ` +
      "Delete application/styles/token-override.css, its two imports and this test " +
      "so the tokens have one source of truth again.",
  );
});

test("the brand palette follows the semantic tokens rather than copying them", () => {
  const local = readFileSync(override, "utf8");
  // `bg-main-card` is generated from @theme at build time while `bg-card` reads :root. When
  // the palette carried its own literal copy the two drifted the moment only one was
  // overridden — dashboard surfaces moved to the new palette and profile/settings/wallet
  // stayed on the old one. Pointing it at the semantic token is what keeps them in step.
  assert.match(local, /--color-main-card:\s*var\(--card\)/, "--color-main-card must reference --card, not repeat its value");
  assert.match(local, /--color-main-surface:\s*var\(--secondary\)/, "--color-main-surface must reference --secondary");
});

test("the element remote is built against the same tokens as the app", () => {
  // The account-chip remote compiles from its own stylesheet, so anything the app-only
  // entry point carries is invisible to it — the chip rendered the old palette against a
  // host that had already moved.
  assert.match(readFileSync(mfeEntry, "utf8"), /token-override\.css/, "mfe.css must import the same override the app does");
});

test("the remote's theme import stays unlayered", () => {
  // Layering the theme import hides the spacing scale from the utility generator: fractional
  // steps compile to nothing, so `size-8.5` / `gap-2.5` vanished and the chip collapsed.
  // Integer steps kept working, which is what made it read as a layout bug.
  assert.match(readFileSync(mfeEntry, "utf8"), /@import "tailwindcss\/theme\.css";/, "the theme import must not carry a layer()");
});
