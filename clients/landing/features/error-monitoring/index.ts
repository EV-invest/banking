/**
 * @module features/error-monitoring
 *
 * Client-side error capture — single entry point for error reporting.
 *
 * **Public API**
 * - `ErrorMonitoringProvider` — mount once in the root layout; boots the SDK.
 * - `ErrorBoundary` — React error boundary that reports caught errors here.
 * - `reportError(error, context?)` — capture an unexpected error from a Client Component.
 *
 * **Rules**
 * - Never import `@sentry/nextjs` outside this feature — init and capture live here.
 * - Server-side errors are captured automatically via `instrumentation.ts` and
 *   the tracing integration; this feature covers React client errors only.
 */
export { ErrorMonitoringProvider } from "./provider";
export { ErrorBoundary } from "./error-boundary";
export { reportError } from "./client";
