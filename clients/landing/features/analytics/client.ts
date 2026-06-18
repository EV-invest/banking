"use client";

import posthog from "posthog-js";

/**
 * @module features/analytics/client
 *
 * PostHog event sink — the only place in the codebase that imports `posthog-js`.
 *
 * **Why an event sink?**
 * Variant assignment is owned by `proxy.ts` + cookies (see `PATTERNS.md` and
 * `shared/config/experiments.ts`). PostHog only *records* what happened; it
 * never *decides* what to show. That keeps bucketing deterministic, SSR-safe,
 * and independent of a third-party network call on the render path.
 *
 * **Init strategy — lazy + idempotent.**
 * `ensure()` runs on first use, so `capture()` is safe regardless of React
 * effect ordering (child effects fire before parent effects). When
 * `NEXT_PUBLIC_POSTHOG_KEY` is unset, every call is a no-op — local dev and
 * tests stay silent while bucketing still works.
 */

let initialized = false;

function ensure(): boolean {
  if (initialized) return true;
  const key = process.env.NEXT_PUBLIC_POSTHOG_KEY;
  if (!key) return false;
  posthog.init(key, {
    api_host:
      process.env.NEXT_PUBLIC_POSTHOG_HOST ?? "https://us.i.posthog.com",
    capture_pageview: true,
    person_profiles: "identified_only",
  });
  initialized = true;
  return true;
}

/**
 * Boots PostHog and fires the initial pageview.
 *
 * Idempotent — safe to call multiple times. Called once from
 * {@link PostHogProvider} on mount. Also ensures `capture()` works before the
 * provider has mounted (child effects fire before parent effects in React).
 *
 * No-op when `NEXT_PUBLIC_POSTHOG_KEY` is unset.
 */
export function initAnalytics(): void {
  ensure();
}

/**
 * Records a product event in PostHog.
 *
 * For A/B exposure and section CTA clicks, prefer {@link useExperimentEvent}
 * from `features/ab-variant` — it scopes the name and merges `variant`
 * automatically. Use `capture` for one-off events unrelated to an experiment.
 *
 * **Naming:** `snake_case`, scoped `<surface>_<thing>_<action>`.
 * Names are the analytics contract — renames break dashboards.
 * Examples: `hero_cta_clicked`, `calculator_submitted`.
 *
 * **Props:** primitive values only (`string`, `number`, `boolean`).
 * No PII — never names, emails, or free-text the user typed.
 *
 * No-op when `NEXT_PUBLIC_POSTHOG_KEY` is unset.
 *
 * @param event - Snake-case event name.
 * @param props - Optional primitive, non-PII payload.
 */
export function capture(
  event: string,
  props?: Record<string, unknown>,
): void {
  if (!ensure()) return;
  posthog.capture(event, props);
}
