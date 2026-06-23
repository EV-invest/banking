import type { Metadata } from "next";
import type { ReactNode } from "react";

import "@/application/styles/globals.css";
import { Providers } from "@/application/providers";

export const metadata: Metadata = {
  title: "EV Investment — Cabinet",
  description: "Your investor cabinet — portfolio, funds and wallet.",
};

// Root layout: html/body + cross-cutting providers only. The visible chrome belongs to
// the route groups — the signed-in app shell (sidebar) lives in `(app)`, the centered
// auth framing in `(auth)`.
export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className="dark" suppressHydrationWarning>
      <body className="min-h-screen bg-background text-foreground antialiased">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
