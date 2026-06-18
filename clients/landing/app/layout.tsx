import type { ReactNode } from "react";
import { cookies } from "next/headers";
import Script from "next/script";
import { fontInter, fontPlayfair } from "@/application/styles/fonts";
import { Providers } from "@/application/providers";
import { ErrorMonitoringProvider } from "@/features/error-monitoring";
import { Header, Footer } from "@/application/layout";
import { PostHogProvider } from "@/features/analytics";
import { DevAbPanel } from "@/features/ab-variant";
import "@/application/styles/globals.css";

export { metadata, viewport } from "@/application/metadata";

const analyticsEndpoint = process.env.NEXT_PUBLIC_ANALYTICS_ENDPOINT;
const analyticsWebsiteId = process.env.NEXT_PUBLIC_ANALYTICS_WEBSITE_ID;

export default async function RootLayout({ children }: { children: ReactNode }) {
  // Reading cookies opts every route into dynamic rendering — required for
  // cookie-based A/B (the assigned variant must be read per request).
  await cookies();

  return (
    <html
      lang="en"
      className={`dark ${fontInter.variable} ${fontPlayfair.variable}`}
      suppressHydrationWarning
    >
      <body>
        <ErrorMonitoringProvider />
        <Providers>
          <PostHogProvider>
            <Header />
            {children}
            <Footer />
          </PostHogProvider>
          <DevAbPanel />
        </Providers>
        {analyticsEndpoint && analyticsWebsiteId && (
          <Script
            defer
            src={`${analyticsEndpoint}/umami`}
            data-website-id={analyticsWebsiteId}
            strategy="afterInteractive"
          />
        )}
      </body>
    </html>
  );
}
