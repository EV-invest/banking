/**
 * @module features/analytics
 *
 * Product instrumentation — single entry point for the analytics event sink.
 *
 * **Public API**
 * - `PostHogProvider` — mount once in the root layout; boots the SDK.
 * - `capture(event, props)` — record any event from a Client Component.
 *
 * **For A/B exposure + section CTA tracking**, use `ExperimentTracker` and
 * `useExperimentEvent` from `features/ab-variant`. They call `capture`
 * internally and scope the event name and variant automatically.
 *
 * **Rules**
 * - Never import `posthog-js` outside this feature — swap/consent/init live here.
 * - Analytics records; it never decides what to render (bucketing = `proxy.ts`).
 * - Client Components only — do not import from Server Components or `next/headers`.
 */
export { PostHogProvider } from "./posthog-provider";
export { capture } from "./client";
