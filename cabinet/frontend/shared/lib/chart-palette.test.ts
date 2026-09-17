// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// There is no canvas here, so the probe is a stand-in: it records what was assigned to
// `fillStyle` and answers `getImageData` with whatever bytes the case says the paint
// produced. What is under test is the translation of those bytes, and every way the
// probe can fail.
import assert from "node:assert/strict";
import test from "node:test";

import { readTokenColors, toEngineColor, type PixelProbe } from "./chart-palette.ts";

interface Fake {
  probe: PixelProbe;
  painted: string[];
}

function fakeProbe(
  pixel: number[],
  options: { rejects?: string; readThrows?: boolean } = {},
): Fake {
  const painted: string[] = [];
  let style = "";
  const probe = {
    get fillStyle() {
      return style;
    },
    set fillStyle(next: string | CanvasGradient | CanvasPattern) {
      // The real getter re-serialises in its own dialect; the sentinel comparison must not
      // depend on the assigned string surviving verbatim.
      if (typeof next === "string" && next !== options.rejects) style = `serialised:${next}`;
    },
    clearRect() {},
    fillRect() {
      painted.push(style);
    },
    getImageData() {
      if (options.readThrows) throw new DOMException("tainted", "SecurityError");
      return { data: Uint8ClampedArray.from(pixel) };
    },
  };
  return { probe, painted };
}

test("an opaque rgb() comes back as rgba() with alpha 1", () => {
  const { probe, painted } = fakeProbe([230, 225, 211, 255]);
  assert.equal(toEngineColor(probe, "rgb(230, 225, 211)", "#9a9a9a"), "rgba(230, 225, 211, 1)");
  assert.deepEqual(painted, ["serialised:rgb(230, 225, 211)"]);
});

test("a color(srgb …) the getter hands back verbatim is read through the pixel instead", () => {
  // `--color-ink-soft` in Chrome 152: the mix of `--ink` at 65 % that the engine could not
  // parse (#317). 0.65 lands on byte 166, which is 0.651 at the byte's resolution.
  const { probe } = fakeProbe([230, 225, 211, 166]);
  assert.equal(
    toEngineColor(probe, "color(srgb 0.901961 0.882353 0.827451 / 0.65)", "#9a9a9a"),
    "rgba(230, 225, 211, 0.651)",
  );
});

test("a fully transparent pixel is still a colour, not a failure", () => {
  const { probe } = fakeProbe([0, 0, 0, 0]);
  assert.equal(toEngineColor(probe, "rgba(0, 0, 0, 0)", "transparent"), "rgba(0, 0, 0, 0)");
});

test("no context, no computed value, a rejected colour or a failed read all fall back", () => {
  assert.equal(toEngineColor(null, "rgb(1, 2, 3)", "#9a9a9a"), "#9a9a9a");
  assert.equal(toEngineColor(fakeProbe([1, 2, 3, 255]).probe, "", "#9a9a9a"), "#9a9a9a");
  const rejecting = fakeProbe([1, 2, 3, 255], { rejects: "nonsense" });
  assert.equal(toEngineColor(rejecting.probe, "nonsense", "#9a9a9a"), "#9a9a9a");
  assert.deepEqual(rejecting.painted, [], "nothing is painted for a colour the canvas refused");
  assert.equal(
    toEngineColor(fakeProbe([1, 2, 3, 255], { readThrows: true }).probe, "rgb(1, 2, 3)", "#9a9a9a"),
    "#9a9a9a",
  );
});

test("without a document every token is its fallback", () => {
  assert.equal(typeof document, "undefined");
  const tokens = { background: "--color-card", text: "--color-ink-soft" };
  const fallback = { background: "transparent", text: "#9a9a9a" };
  const palette = readTokenColors(tokens, fallback);
  assert.deepEqual(palette, fallback);
  assert.notEqual(palette, fallback, "a copy, so a caller mutating its palette cannot rewrite the fallback");
});
