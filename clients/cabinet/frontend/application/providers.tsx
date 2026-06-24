"use client";

import { PostHogProvider } from "@evinvest/analytics/react";
import { ErrorMonitoringProvider } from "@evinvest/error-monitoring/react";
import { ThemeProvider } from "next-themes";
import type { ReactNode } from "react";

// Client observability providers wrap the tree. Both read their config from
// NEXT_PUBLIC_* env at runtime and no-op when unset (no DSN / no key), so the
// same tree renders unconfigured in local dev and CI. `nonce` is the per-request
// CSP nonce (from the root layout) so next-themes' inline script stays allowed.
export function Providers({ children, nonce }: { children: ReactNode; nonce?: string }) {
  return (
    <ErrorMonitoringProvider>
      <PostHogProvider>
        <ThemeProvider attribute="class" forcedTheme="dark" enableSystem={false} nonce={nonce}>
          {children}
        </ThemeProvider>
      </PostHogProvider>
    </ErrorMonitoringProvider>
  );
}
