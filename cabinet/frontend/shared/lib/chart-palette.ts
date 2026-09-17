// A canvas chart's colours, read from the uikit's tokens at mount rather than written here.
//
// A plotting engine paints on a canvas, so it cannot read a CSS custom property itself —
// and the tokens are not plain hex: `--color-border` and `--color-ink-soft` are a
// `color-mix()` over `--ink`, which `getPropertyValue` returns as the unevaluated
// expression and no canvas parser accepts. So each token is resolved the way the browser
// would resolve it on an element — a probe with `color: var(--token)`, read back through
// `getComputedStyle` — and then normalised by painting it into a 1×1 canvas and reading
// the pixel back as `rgba()`.
//
// The pixel, not the context's `fillStyle` getter: that getter hands a mixed colour back
// as `color(srgb …)` in current Chrome, verbatim, and the engine's own parser stops at
// `rgb()`/`rgba()`/`#hex` — the trade terminal fell over on exactly that (#317). Bytes
// have no such dialect.
//
// Which tokens make up a palette is the chart's business (`views/trade/lib/theme-colors.ts`,
// `views/dashboard/lib/use-perf-chart.ts`); this module only knows how to resolve one.
// Deliberately import-free so the Node test runner can load it without the `@/` alias.

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

/**
 * What a chart wears where a token could not be resolved — no document yet, a canvas that
 * refuses to paint. The one place a hex is justified in the cabinet: the engine paints
 * bytes, and a fallback is by definition what is used when no token is readable.
 *
 * `text` and `grid` are deliberately dull, so a missing token reads as "not themed"
 * rather than as a design choice; the named tints are the values `tokens.css` ships for
 * `--chart-2` / `--positive`, `--chart-3` and `--accent-error`, so a series that fell
 * back is the same colour as its legend dot, never a third tint.
 */
export const ENGINE_FALLBACK = {
  transparent: "transparent",
  text: "#9a9a9a",
  grid: "#2a2a2a",
  /** `--color-secondary`: the solid plate a crosshair label sits on. */
  plate: "#0d1526",
  /** `--chart-2`, also `--positive`. */
  green: "#2e9e5b",
  /** `--chart-3`. */
  yellow: "#f2c94c",
  /** `--accent-error`. */
  red: "#ef5b52",
} as const;

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

/**
 * Resolve each named token (`--color-card`, …) to a colour the engine accepts, or to its
 * fallback where the probe cannot run — no document yet, a canvas that refuses to paint.
 */
export function readTokenColors<K extends string>(tokens: Readonly<Record<K, string>>, fallback: Readonly<Record<K, string>>): Record<K, string> {
  if (typeof document === "undefined") return { ...fallback };
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
    const palette: Record<K, string> = { ...fallback };
    for (const key of Object.keys(tokens) as K[]) {
      probe.style.color = `var(${tokens[key]})`;
      palette[key] = toEngineColor(ctx, getComputedStyle(probe).color, fallback[key]);
    }
    return palette;
  } finally {
    probe.remove();
  }
}
