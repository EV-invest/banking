"use client";

// The candle chart's colours, read from the uikit's tokens at mount rather than written
// here. The resolving itself — probe, canvas, `rgba()` — is `shared/lib/chart-palette.ts`;
// this file only says which tokens the terminal's chart is made of: `positive` for a
// rising bar, `accent-error` for a falling one, `card` for the pane it sits in.

import { readTokenColors } from "../../../shared/lib/chart-palette.ts";

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
  return readTokenColors(TOKENS, FALLBACK);
}
