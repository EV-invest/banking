"use client";

import type { ReactNode } from "react";
import { ThemeProvider } from "next-themes";
import { ErrorBoundary } from "@/features/error-monitoring";
import { Toaster, TooltipProvider } from "@evinvest/uikit";

// Global client providers. The site is intentionally dark-only, so the theme is
// forced rather than switchable (next-themes still owns the `class` on <html>).
export function Providers({ children }: { children: ReactNode }) {
  return (
    <ErrorBoundary>
      <ThemeProvider attribute="class" forcedTheme="dark" enableSystem={false}>
        <TooltipProvider>
          <Toaster />
          {children}
        </TooltipProvider>
      </ThemeProvider>
    </ErrorBoundary>
  );
}
