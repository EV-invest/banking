// What the price chart draws over its canvas, decided apart from the JSX so the rule can be
// tested without a DOM. A failed engine chunk is told apart from any other failure: the
// bundler will not fetch that chunk again without a reload, so "please try again" would be
// an instruction nothing obeys.

import type { ChartState } from "@/views/trade/lib/use-candle-chart";

import { isChunkLoadError } from "../../../shared/lib/chunk-load-error.ts";

export type ChartOverlay = { kind: "none" } | { kind: "reload" } | { kind: "loading" } | { kind: "empty" } | { kind: "error"; error: unknown };

export function chartOverlay(state: ChartState): ChartOverlay {
  switch (state.kind) {
    case "ready":
      return { kind: "none" };
    case "loading":
    case "empty":
      return { kind: state.kind };
    case "failed":
      return isChunkLoadError(state.error) ? { kind: "reload" } : { kind: "error", error: state.error };
  }
}
