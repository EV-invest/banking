// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// These guard two failures that both shipped once and neither of which announced itself:
// the palette drifting apart from the semantic tokens, and the account-chip remote being
// built from a stylesheet that silently drops half of what it needs.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import test from "node:test";

// Resolved through the package's own exports map, so this tracks whatever version is
// installed rather than a guessed path into node_modules. The `main-*` palette this test
// is about is not in it: since uikit 0.10 `tokens.css` carries the renamed vocabulary
// only, and 0.14 dropped the legacy sheet, so the aliases live in the cabinet's own
// layer, which both entry points import after the kit's (see globals.css).
const uikitTokens = createRequire(import.meta.url).resolve("@evinvest/uikit/styles/tokens.css");
const legacyTokens = new URL("../../application/styles/tokens-legacy.css", import.meta.url);
const mfeEntry = new URL("../../mfe/account-chip/mfe.css", import.meta.url);

test("the brand palette follows the semantic tokens rather than copying them", () => {
  // `bg-main-card` is generated from @theme at build time while `bg-card` reads :root.
  // While the palette carried its own literal copies the two drifted the moment only one
  // side was retuned — dashboard surfaces moved to the new palette and profile, settings
  // and wallet stayed on the old one, so half the cabinet changed colour and half did not.
  const upstream = readFileSync(uikitTokens, "utf8");
  const legacy = readFileSync(legacyTokens, "utf8");
  assert.match(upstream, /^\s*--card:/m, "the kit's sheet must still define --card");
  assert.match(upstream, /^\s*--secondary:/m, "the kit's sheet must still define --secondary");
  assert.match(legacy, /--color-main-card:\s*var\(--card\)/, "--color-main-card must reference --card, not repeat its value");
  assert.match(legacy, /--color-main-surface:\s*var\(--secondary\)/, "--color-main-surface must reference --secondary");
});

test("the element remote is built against the shared tokens", () => {
  // The remote compiles from its own stylesheet, so it does not inherit anything the app
  // entry point sets up. Without this import it renders an untokenised palette.
  const remote = readFileSync(mfeEntry, "utf8");
  assert.match(remote, /@evinvest\/uikit\/styles\/tokens\.css/, "mfe.css must import the same token sheet as globals.css");
  assert.match(remote, /application\/styles\/tokens-legacy\.css/, "mfe.css must import the same legacy alias layer as globals.css");
});

test("the remote's theme import stays unlayered", () => {
  // Layering the theme import hides the spacing scale from the utility generator, so
  // fractional steps compile to nothing: `size-8.5` and `gap-2.5` vanished and the chip
  // collapsed, while integer steps kept working — which is what made it read as a layout
  // bug rather than a build one.
  assert.match(readFileSync(mfeEntry, "utf8"), /@import "tailwindcss\/theme\.css";/, "the theme import must not carry a layer()");
});
