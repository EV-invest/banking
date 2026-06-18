"use client";

/**
 * Boots the client-side error monitoring SDK.
 *
 * Mount once in the root layout. Turbopack does not support webpack
 * entry-point injection, so `withSentryConfig` cannot auto-import
 * `sentry.client.config` — this provider does it explicitly instead.
 *
 * Renders nothing. No-op when `NEXT_PUBLIC_SENTRY_DSN` is unset.
 */
import "@/sentry.client.config";

export function ErrorMonitoringProvider() {
  return null;
}
