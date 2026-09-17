"use client";

// The candle chart's colours, read from the uikit's tokens at mount rather than written
// here. The resolving itself — probe, canvas, `rgba()` — is `shared/lib/chart-palette.ts`;
// this file only says which tokens the terminal's chart is made of: `positive` for a
// rising bar, `accent-error` for a falling one, `card` for the pane it sits in.

import { ENGINE_FALLBACK, readTokenColors } from "../../../shared/lib/chart-palette.ts";

export interface ChartPalette {
  background: string;
  text: string;
  grid: string;
  up: string;
  down: string;
}

const FALLBACK: ChartPalette = { background: ENGINE_FALLBACK.transparent, text: ENGINE_FALLBACK.text, grid: ENGINE_FALLBACK.grid, up: ENGINE_FALLBACK.green, down: ENGINE_FALLBACK.red };

const TOKENS: Record<keyof ChartPalette, string> = {
  background: "--color-card",
  text: "--color-ink-soft",
  grid: "--color-border",
  up: "--color-positive",
  down: "--color-accent-error",
};

export function readChartPalette(): ChartPalette {
  return readTokenColors(TOKENS, FALLBACK);
}
