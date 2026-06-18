/**
 * Sentry client-side (browser) initialisation.
 *
 * Loaded by {@link features/error-monitoring/provider.tsx} on every page render.
 * Turbopack cannot inject this via webpack entry points, so the provider
 * imports it explicitly.
 *
 * - `tracesSampleRate` — 10 % in production to limit transaction volume;
 *   100 % in development so every request is visible while debugging.
 * - `replaysOnErrorSampleRate` — always record a replay when an error fires,
 *   regardless of the session sample rate.
 * - `replaysSessionSampleRate` — record 5 % of normal sessions for UX insight.
 * - `maskAllText: false` — text is visible in replays; add the `sentry-mask`
 *   CSS class to any element that must be redacted (passwords, PII fields).
 */
import * as Sentry from "@sentry/nextjs";

Sentry.init({
  dsn: process.env.NEXT_PUBLIC_SENTRY_DSN,
  environment: process.env.NEXT_PUBLIC_APP_ENV ?? "development",
  tracesSampleRate: process.env.NODE_ENV === "production" ? 0.1 : 1.0,
  replaysOnErrorSampleRate: 1.0,
  replaysSessionSampleRate: 0.05,
});

// replayIntegration is browser-only — not available in the server bundle.
if (typeof window !== "undefined") {
  Sentry.addIntegration(
    Sentry.replayIntegration({
      maskAllText: false,
      blockAllMedia: false,
    }),
  );
}
