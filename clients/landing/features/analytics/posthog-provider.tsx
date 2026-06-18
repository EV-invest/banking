"use client";

import { useEffect, type ReactNode } from "react";
import { initAnalytics } from "./client";

/**
 * Boots PostHog on the client and records the initial pageview.
 *
 * Mount **once** in the root layout, above all sections. Renders children
 * untouched. No-op when `NEXT_PUBLIC_POSTHOG_KEY` is unset — safe in local
 * and test environments without any configuration.
 *
 * Does **not** expose context. Sections fire events through
 * {@link useExperimentEvent} (from `features/ab-variant`) or {@link capture}
 * directly — this provider only ensures the SDK is initialised first.
 */
export function PostHogProvider({ children }: { children: ReactNode }) {
  useEffect(() => {
    initAnalytics();
  }, []);
  return <>{children}</>;
}
