// Which state the dashboard's performance plot is in, decided apart from the JSX so the
// order of the checks can be tested without a DOM. The order is the contract: one skeleton
// until both the history and (for a plot with something to draw) the engine are here; a
// failed history read before an empty one; an empty series before the engine's verdict,
// because nothing to draw needs no engine.

import type { PerfEngineState } from "@/views/dashboard/lib/use-perf-chart";

import { isChunkLoadError } from "../../../shared/lib/chunk-load-error.ts";

export interface PerfViewInput {
  /** The allocation whose log is drawn; `null` while the dashboard is still resolving which. */
  allocation: string | null;
  /** The history read has nothing to show yet (the resource's `isLoading`). */
  historyLoading: boolean;
  /** The history read failed with no data to fall back on. */
  historyFailed: boolean;
  /** The series has at least one mark to plot. */
  drawable: boolean;
  engine: PerfEngineState;
}

export type PerfView = { kind: "skeleton" } | { kind: "history-error" } | { kind: "empty" } | { kind: "reload" } | { kind: "engine-error"; error: unknown } | { kind: "plot" };

export function perfView({ allocation, historyLoading, historyFailed, drawable, engine }: PerfViewInput): PerfView {
  if (allocation === null || historyLoading || (drawable && engine.kind === "loading")) return { kind: "skeleton" };
  if (historyFailed) return { kind: "history-error" };
  if (!drawable) return { kind: "empty" };
  if (engine.kind === "failed") return isChunkLoadError(engine.error) ? { kind: "reload" } : { kind: "engine-error", error: engine.error };
  return { kind: "plot" };
}
