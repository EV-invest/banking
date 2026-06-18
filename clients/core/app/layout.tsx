import type { Metadata } from "next";
import type { ReactNode } from "react";

import "@/application/styles/globals.css";
import { Header } from "@/application/layout/header";
import { Providers } from "@/application/providers";

export const metadata: Metadata = {
  title: "EV Banking — Console",
  description: "Host shell composing the bank's microfrontends.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className="dark" suppressHydrationWarning>
      <body className="min-h-screen bg-background text-foreground antialiased">
        <Providers>
          <Header />
          <main>{children}</main>
        </Providers>
      </body>
    </html>
  );
}
