/**
 * Next.js instrumentation hook — runs once per server process on startup.
 *
 * Loads the correct Sentry config for the active runtime:
 * - `nodejs`  → `sentry.server.config.ts` (API routes, RSC, SSR)
 * - `edge`    → `sentry.edge.config.ts`   (`proxy.ts` middleware)
 *
 * `onRequestError` wires Sentry into Next.js's built-in server-error hook
 * so unhandled errors in Server Components and Route Handlers are captured
 * automatically without wrapping each handler manually.
 */
import * as Sentry from "@sentry/nextjs";

export async function register() {
  if (process.env.NEXT_RUNTIME === "nodejs") {
    await import("./sentry.server.config");
  }
  if (process.env.NEXT_RUNTIME === "edge") {
    await import("./sentry.edge.config");
  }
}

export const onRequestError = Sentry.captureRequestError;
