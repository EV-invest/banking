import type { Metadata } from "next";
import type { ReactNode } from "react";

import "@/application/styles/globals.css";
import { Providers } from "@/application/providers";
import { fontInter, fontPlayfair } from "@/application/styles/fonts";
import { requestNonce } from "@/shared/config/security";

export const metadata: Metadata = {
  title: "EV Investment — Cabinet",
  description: "Your investor cabinet — portfolio, funds and wallet.",
};

// Root layout: html/body + cross-cutting providers only. The visible chrome belongs to
// the route groups — the signed-in app shell (sidebar) lives in `(app)`, the centered
// auth framing in `(auth)`.
export default async function RootLayout({ children }: { children: ReactNode }) {
  const nonce = (await requestNonce()) ?? undefined;
  return (
    <html
      lang="en"
      className={`dark ${fontInter.variable} ${fontPlayfair.variable}`}
      suppressHydrationWarning
    >
      <body className="min-h-screen bg-background text-foreground antialiased">
        <Providers nonce={nonce}>{children}</Providers>
      </body>
    </html>
  );
}
