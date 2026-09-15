"use client";

// The chart's colours, read from the uikit's tokens at mount rather than written here.
//
// The engine paints on a canvas, so it cannot read a CSS custom property itself — and the
// tokens are not plain hex: `--color-border` and `--color-ink-soft` are a `color-mix()`
// over `--ink`, which `getPropertyValue` returns as the unevaluated expression and no
// canvas parser accepts. So each token is resolved the way the browser would resolve it
// on an element — a probe with `color: var(--token)`, read back through
// `getComputedStyle` — and then normalised by painting it into a 1×1 canvas and reading
// the pixel back as `rgba()`.
//
// The pixel, not the context's `fillStyle` getter: that getter hands a mixed colour back
// as `color(srgb …)` in current Chrome, verbatim, and the engine's own parser stops at
// `rgb()`/`rgba()`/`#hex` — the trade terminal fell over on exactly that (#317). Bytes
// have no such dialect.
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

// The slice of a 2D context the normaliser touches, so a test can stand one in where
// there is no canvas at all (Node's runner has no DOM).
export interface PixelProbe {
  fillStyle: string | CanvasGradient | CanvasPattern;
  clearRect(x: number, y: number, w: number, h: number): void;
  fillRect(x: number, y: number, w: number, h: number): void;
  getImageData(x: number, y: number, w: number, h: number): { data: Uint8ClampedArray };
}

// A colour the canvas cannot parse leaves `fillStyle` untouched; this is what it is left
// at, and it is compared by the getter's own rendering so the getter's dialect does not
// matter. Not a colour any token resolves to.
const SENTINEL = "#010203";

// Normalises a computed colour into the `rgba(r, g, b, a)` the engine accepts, by way of
// one painted pixel. Alpha keeps three decimals — the byte's full resolution (1/255) — so
// nothing is lost past what the canvas itself quantised. Anything the probe cannot do
// (no context, a colour it rejects, a read that throws) yields the fallback.
export function toEngineColor(ctx: PixelProbe | null, computed: string, fallback: string): string {
  if (!ctx || !computed) return fallback;
  try {
    ctx.fillStyle = SENTINEL;
    const unparsed = ctx.fillStyle;
    ctx.fillStyle = computed;
    if (ctx.fillStyle === unparsed) return fallback;
    ctx.clearRect(0, 0, 1, 1);
    ctx.fillRect(0, 0, 1, 1);
    const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
    if (r === undefined || g === undefined || b === undefined || a === undefined) return fallback;
    return `rgba(${r}, ${g}, ${b}, ${Math.round((a / 255) * 1000) / 1000})`;
  } catch {
    return fallback;
  }
}

export function readChartPalette(): ChartPalette {
  if (typeof document === "undefined") return FALLBACK;
  const canvas = document.createElement("canvas");
  canvas.width = 1;
  canvas.height = 1;
  // `willReadFrequently` keeps the 1×1 surface in software, which is what a read-back of
  // every pixel wants anyway and spares a GPU round-trip per token.
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  const probe = document.createElement("span");
  probe.hidden = true;
  document.body.appendChild(probe);
  try {
    const resolve = (token: string, fallback: string): string => {
      probe.style.color = `var(${token})`;
      return toEngineColor(ctx, getComputedStyle(probe).color, fallback);
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
