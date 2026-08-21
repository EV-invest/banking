import type { Metadata } from "next";
import type { ReactNode } from "react";

import { I18nProvider } from "@evinvest/i18n/react";

import "@/application/styles/globals.css";
import { Providers } from "@/application/providers";
import { fontInter } from "@/application/styles/fonts";
import { isLocale } from "@evinvest/i18n";
import { notFound } from "next/navigation";
import { messagesFor } from "@/shared/config/i18n";
import { requestNonce } from "@/shared/config/security";

export const metadata: Metadata = {
  title: "EV Investment — Cabinet",
  description: "Your investor cabinet — portfolio, funds and wallet.",
};

// Root layout: html/body + cross-cutting providers only. The visible chrome belongs to
// the route groups — the signed-in app shell (sidebar) lives in `(app)`, the centered
// auth framing in `(auth)`.
//
// This is the ROOT layout despite sitting under `[locale]`, and deliberately so:
// `[locale]` has to be a *root* param for `next/root-params` to expose it to the
// server components that need the locale but are handed no props (the status
// pages, `currentLocale()`). An `app/layout.tsx` above this one would demote it.
// Same shape as site_conductor.
export default async function RootLayout({
  children,
  params,
}: {
  children: ReactNode;
  params: Promise<{ locale: string }>;
}) {
  const nonce = (await requestNonce()) ?? undefined;
  // The locale is the URL now, not a cookie. `(auth)` needs it as much as `(app)`
  // — someone who cannot yet sign in is exactly the reader who should not be met
  // in a language they do not read.
  const { locale: raw } = await params;
  if (!isLocale(raw)) notFound();
  const locale = raw;
  return (
    <html lang={locale} className={`dark ${fontInter.variable}`} suppressHydrationWarning>
      <body className="min-h-screen bg-background text-foreground antialiased">
        <I18nProvider locale={locale} messages={messagesFor(locale)}>
          <Providers nonce={nonce}>{children}</Providers>
        </I18nProvider>
      </body>
    </html>
  );
}
