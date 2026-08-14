"use client";

// What each screen reads, so the reading can start before the screen does.
//
// The cache in `shared/lib/resource.ts` removes the skeleton on the SECOND visit to a
// screen — the value is already there. This closes the first visit too. `next/link` already
// prefetches the route's code and RSC payload on hover; this is the same idea for the data
// that code will ask for, driven off the same signal. By the time the click lands the answer
// is usually in the cache, and the screen paints its figures on the first frame.
//
// Two triggers, both cheap and both cancel-free (a warm that turns out to be unnecessary
// costs one GET that a later screen would have made anyway):
//
//   · intent — pointer or keyboard focus on a nav row warms that row's screen;
//   · idle — once the shell is up, warm the handful of reads shared by most screens, at
//     idle priority so they never race the reads the current screen is making for itself.

import { allocationsResource, fundNavResource, positionsResource, redemptionsResource } from "@/entities/fund/model/fund-resource";
import { notificationSettingsResource, notificationsResource } from "@/entities/notification/model/notification-resource";
import { RECENT_OPS, operationsResource } from "@/entities/operation/model/operation-resource";
import { sessionsResource } from "@/entities/session/model/session-resource";
import { profileResource } from "@/entities/user/model/profile-resource";
import { depositsResource, walletResource, withdrawalsResource } from "@/entities/wallet/model/wallet-resource";

// Zone-relative paths, exactly as the rail writes them (basePath is not part of these).
// Ordered longest-prefix-first so `/wallet/activity` isn't answered by `/wallet`.
const ROUTES: ReadonlyArray<{ prefix: string; warm: (path: string) => void }> = [
  {
    prefix: "/wallet/activity",
    warm: () => {
      withdrawalsResource.prefetch();
      depositsResource.prefetch();
    },
  },
  { prefix: "/wallet", warm: () => walletResource.prefetch() },
  {
    prefix: "/invest/",
    warm: (path) => {
      allocationsResource.prefetch();
      positionsResource.prefetch();
      redemptionsResource.prefetch();
      // `/invest/<service>` — the fund's own page opens on its NAV.
      const service = decodeURIComponent(path.slice("/invest/".length).split("/")[0] ?? "");
      if (service) fundNavResource.prefetch(service);
    },
  },
  {
    prefix: "/invest",
    warm: () => {
      positionsResource.prefetch();
      redemptionsResource.prefetch();
      walletResource.prefetch();
      allocationsResource.prefetch();
    },
  },
  {
    prefix: "/operations",
    warm: () => {
      operationsResource.prefetch(undefined);
      allocationsResource.prefetch();
    },
  },
  {
    prefix: "/notifications",
    warm: () => notificationsResource.prefetch("all"),
  },
  {
    prefix: "/profile",
    warm: () => {
      profileResource.prefetch();
      positionsResource.prefetch();
    },
  },
  {
    prefix: "/settings",
    warm: () => {
      profileResource.prefetch();
      sessionsResource.prefetch();
      notificationSettingsResource.prefetch();
    },
  },
  {
    prefix: "/",
    warm: () => {
      walletResource.prefetch();
      positionsResource.prefetch();
      operationsResource.prefetch(RECENT_OPS);
      allocationsResource.prefetch();
    },
  },
];

// "/" is Home, not "every path": it is matched exactly, never as a prefix. Every other entry
// matches its own path or anything nested under it.
function matches(path: string, prefix: string): boolean {
  if (prefix === "/") return path === "/";
  return path === prefix || path.startsWith(prefix.endsWith("/") ? prefix : `${prefix}/`);
}

/** Warm the reads the screen at `href` will make. Safe to call repeatedly — fresh entries are left alone. */
export function prefetchRoute(href: string): void {
  const path = href.split("?")[0] ?? href;
  ROUTES.find((route) => matches(path, route.prefix))?.warm(path);
}

/**
 * Handlers to spread onto a nav link so intent warms its screen. `onFocus` is not optional
 * garnish: a keyboard user tabbing the rail has to get the same head start a pointer does.
 */
export function prefetchOn(href: string) {
  const warm = () => prefetchRoute(href);
  return { onMouseEnter: warm, onFocus: warm, onTouchStart: warm };
}

/**
 * Warm what nearly every screen in the cabinet reads: the balance, the positions, and the
 * fund catalog the rail itself lists. Called once when the signed-in shell mounts.
 *
 * At idle, so the current screen's own reads go first — nothing here is needed to paint the
 * page the user actually asked for.
 */
export function warmShell(): () => void {
  const warm = () => {
    walletResource.prefetch();
    positionsResource.prefetch();
    allocationsResource.prefetch();
  };
  if (typeof window === "undefined") return () => undefined;
  if (typeof requestIdleCallback === "function") {
    const handle = requestIdleCallback(warm, { timeout: 2_000 });
    return () => cancelIdleCallback(handle);
  }
  // Safari has no requestIdleCallback; a short timer is close enough for a warm-up.
  const timer = setTimeout(warm, 500);
  return () => clearTimeout(timer);
}
