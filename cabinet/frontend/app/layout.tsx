import type { Metadata } from "next";
import type { ReactNode } from "react";

import { I18nProvider } from "@evinvest/i18n/react";

import "@/application/styles/globals.css";
import { Providers } from "@/application/providers";
import { fontInter } from "@/application/styles/fonts";
import { messagesFor } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";
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
  // Read here rather than in `(app)`: the `(auth)` group needs it too — someone
  // who cannot yet sign in is exactly the reader who should not be met in a
  // language they do not read.
  const locale = await currentLocale();
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
