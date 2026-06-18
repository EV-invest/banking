"use client";

import { ThemeProvider } from "next-themes";
import type { ReactNode } from "react";

export function Providers({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider attribute="class" forcedTheme="dark" enableSystem={false}>
      {children}
    </ThemeProvider>
  );
}
