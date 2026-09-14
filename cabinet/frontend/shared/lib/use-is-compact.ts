"use client";

import { useCallback, useSyncExternalStore } from "react";

// A presentation switch CSS alone cannot make: below the breakpoint a surface is a
// different component — a bottom sheet for a popover, a card list for a table — not the
// same one with different classes, so the query is read at runtime rather than left to a
// media query that cannot swap a component tree.
//
// The server snapshot is `false` (the wide presentation), so a page arriving from the
// server paints wide once and corrects on hydration. That is safe for the surfaces using
// it because their first paint is a loading skeleton — no row, and therefore no trigger,
// exists until the query has settled — and a client-side navigation reads the real value
// on its first render, with no flash at all.
const QUERIES = {
  /** Below Tailwind's `md` — a phone. */
  md: "(max-width: 767px)",
  /** Below `lg` — a phone or a portrait tablet. */
  lg: "(max-width: 1023px)",
} as const;

export type CompactBreakpoint = keyof typeof QUERIES;

export function useIsCompact(below: CompactBreakpoint = "lg"): boolean {
  const query = QUERIES[below];
  const subscribe = useCallback(
    (onChange: () => void) => {
      const list = window.matchMedia(query);
      list.addEventListener("change", onChange);
      return () => list.removeEventListener("change", onChange);
    },
    [query],
  );
  return useSyncExternalStore(
    subscribe,
    () => window.matchMedia(query).matches,
    () => false,
  );
}
