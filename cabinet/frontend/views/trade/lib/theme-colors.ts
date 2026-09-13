"use client";

// The chart's colours, read from the uikit's tokens at mount rather than written here.
//
// The engine paints on a canvas, so it cannot read a CSS custom property itself — and the
// tokens are not plain hex: `--color-border` is a `color-mix()` over `--ink`, which
// `getPropertyValue` returns as the unevaluated expression and no canvas parser accepts.
// So each token is resolved the way the browser would resolve it on an element — a probe
// with `color: var(--token)`, read back through `getComputedStyle` — and then normalised
// through a 2D context's `fillStyle`, which is the one place the platform turns any
// colour it understands into the `rgb()`/`#hex` form the engine understands too.
//
// Token names are the uikit's (`@evinvest/uikit/styles/tokens.css`): `positive` for a
// rising bar, `accent-error` for a falling one, `card` for the pane it sits in.

export interface ChartPalette {
  background: string;
  text: string;
  grid: string;
  up: string;
  down: string;
}

// Fallbacks for a probe that cannot run (no document yet, a canvas that refuses to paint):
// the engine's own defaults on a dark surface, deliberately dull so a missing token reads
// as "not themed" rather than as a design choice.
const FALLBACK: ChartPalette = { background: "transparent", text: "#9a9a9a", grid: "#2a2a2a", up: "#2e9e5b", down: "#ef5b52" };

const TOKENS: Record<keyof ChartPalette, string> = {
  background: "--color-card",
  text: "--color-ink-soft",
  grid: "--color-border",
  up: "--color-positive",
  down: "--color-accent-error",
};

export function readChartPalette(): ChartPalette {
  if (typeof document === "undefined") return FALLBACK;
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  const probe = document.createElement("span");
  probe.hidden = true;
  document.body.appendChild(probe);
  try {
    const resolve = (token: string, fallback: string): string => {
      probe.style.color = `var(${token})`;
      const computed = getComputedStyle(probe).color;
      if (!computed || !ctx) return fallback;
      ctx.fillStyle = "#000";
      ctx.fillStyle = computed;
      // A colour the canvas could not parse leaves the previous value in place.
      return ctx.fillStyle === "#000000" && computed !== "rgb(0, 0, 0)" ? fallback : ctx.fillStyle;
    };
    return {
      background: resolve(TOKENS.background, FALLBACK.background),
      text: resolve(TOKENS.text, FALLBACK.text),
      grid: resolve(TOKENS.grid, FALLBACK.grid),
      up: resolve(TOKENS.up, FALLBACK.up),
      down: resolve(TOKENS.down, FALLBACK.down),
    };
  } finally {
    probe.remove();
  }
}
