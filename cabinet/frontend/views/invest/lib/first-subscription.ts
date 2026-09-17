"use client";

// `first_subscription`: an accepted subscription by an account that held no units. The
// positions are what `/invest/<service>` warms on the way in, so "unread" is rare and is
// counted as "none" — flagged, rather than losing the step. Keyed by user per browser so
// a retry, or a second tab, does not repeat it.

import type { CaptureFn } from "@evinvest/analytics/react";

import { ACTIVATION, once } from "@/shared/analytics";
import { cachedSession } from "@/shared/lib/session";

/** `held`: whether the account held units BEFORE this subscription; `null` when unread. */
export function recordFirstSubscription(capture: CaptureFn, service: string, held: boolean | null): void {
  if (held === true) return;
  const userId = cachedSession()?.user?.userId ?? "anon";
  if (once(`${ACTIVATION.firstSubscription}:${userId}`, "browser")) capture(ACTIVATION.firstSubscription, { service, holdings_known: held !== null });
}
