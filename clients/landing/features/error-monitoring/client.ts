"use client";

import * as Sentry from "@sentry/nextjs";

/**
 * Reports an unexpected error to the error monitoring service.
 *
 * The only place in the codebase that calls Sentry directly — all other code
 * goes through this function so the vendor can be swapped without touching
 * call sites.
 *
 * No-op when `NEXT_PUBLIC_SENTRY_DSN` is unset (local dev without config).
 */
export function reportError(
  error: Error,
  context?: Record<string, unknown>,
): void {
  Sentry.captureException(error, context ? { extra: context } : undefined);
}
