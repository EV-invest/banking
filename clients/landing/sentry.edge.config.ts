/**
 * Sentry Edge Runtime initialisation.
 *
 * Loaded lazily by `instrumentation.ts` when `NEXT_RUNTIME === "edge"`.
 * The Edge Runtime runs `proxy.ts` (A/B cookie assignment). Tracing is
 * disabled here (`tracesSampleRate: 0`) because proxy logic is lightweight
 * and does not benefit from transaction-level performance data.
 */
import * as Sentry from "@sentry/nextjs";

Sentry.init({
  dsn: process.env.SENTRY_DSN,
  tracesSampleRate: 0,
});
