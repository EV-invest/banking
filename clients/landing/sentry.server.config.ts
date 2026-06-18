/**
 * Sentry Node.js server initialisation.
 *
 * Loaded lazily by `instrumentation.ts` when `NEXT_RUNTIME === "nodejs"`.
 * Runs once per server process. Unhandled exceptions and slow requests are
 * captured automatically via the tracing integration.
 */
import * as Sentry from "@sentry/nextjs";

Sentry.init({
  dsn: process.env.SENTRY_DSN,
  environment: process.env.APP_ENV ?? "development",
  tracesSampleRate: process.env.NODE_ENV === "production" ? 0.1 : 1.0,
});
