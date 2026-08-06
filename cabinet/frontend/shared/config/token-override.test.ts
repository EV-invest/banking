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
const globals = new URL("../../application/styles/globals.css", import.meta.url);

/** Last wins, matching the cascade — the override block sits after the import. */
function readToken(css: string, name: string): string | undefined {
  const hits = [...css.matchAll(new RegExp(`^\\s*--${name}:\\s*([^;]+);`, "gm"))];
  return hits.at(-1)?.[1].trim();
}

// The values the override exists to supply. Keep in step with the block itself.
const OVERRIDDEN = ["card", "popover", "secondary", "muted", "muted-foreground", "primary", "primary-foreground", "destructive", "destructive-foreground", "border"];

test("the temporary token override is still doing something", () => {
  const upstream = readFileSync(uikitTokens, "utf8");
  const local = readFileSync(globals, "utf8");

  const redundant = OVERRIDDEN.filter((name) => {
    const ours = readToken(local, name);
    return ours !== undefined && ours === readToken(upstream, name);
  });

  assert.deepEqual(
    redundant,
    [],
    `@evinvest/uikit now ships these tokens itself: ${redundant.join(", ")}. ` +
      "Delete the temporary override block in application/styles/globals.css (and this test) " +
      "so the tokens have one source of truth again.",
  );
});

test("the override actually reaches the brand palette", () => {
  const local = readFileSync(globals, "utf8");
  // `bg-main-card` is generated from @theme at build time, not from :root, so a :root-only
  // override would leave every existing card on the old value — the bug that made the
  // cabinet look flat in the first place.
  assert.match(local, /@theme inline\s*\{[^}]*--color-main-card:/, "the brand palette is re-declared in @theme, not only in :root");
});
