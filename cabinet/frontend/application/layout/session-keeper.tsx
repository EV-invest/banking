"use client";

// Keeps the signed-in shell signed in. Renders nothing.
//
// The shared access cookie carries a short-TTL JWT; the cookie the zone proxy gates on
// outlives it by days (see `shared/lib/session.ts`). Without this, a tab left open — or
// re-entered — past the access TTL renders the page and then fails every `/api/*` call
// with `unauthenticated` until something happens to hit the shell session endpoint. So
// this rotates the cookie on the cadence the TTL needs, and again whenever the tab comes
// back to the user (a hidden tab's timers are throttled or frozen, so returning to it is
// exactly when the cookie is most likely lapsed).
//
// It also owns the ONE navigation this concern needs: when the shell answers that the
// session is genuinely gone, move to /login with a returnTo instead of leaving the user
// on a shell whose every panel reads "sign in again". An unknown answer (offline, 5xx, no
// shell mounted in dev) is never treated as a sign-out.

import { usePathname, useRouter } from "next/navigation";
import { useEffect } from "react";

import { STALE_MS, onSessionChange, refreshIfStale } from "@/shared/lib/session";

export function SessionKeeper() {
  const router = useRouter();
  const pathname = usePathname();

  useEffect(() => {
    const rotate = () => {
      if (document.visibilityState === "visible") void refreshIfStale();
    };
    rotate();
    const timer = setInterval(rotate, STALE_MS);
    document.addEventListener("visibilitychange", rotate);
    window.addEventListener("focus", rotate);
    window.addEventListener("online", rotate);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", rotate);
      window.removeEventListener("focus", rotate);
      window.removeEventListener("online", rotate);
    };
  }, []);

  useEffect(
    () =>
      onSessionChange((session) => {
        if (session.authenticated) return;
        // `pathname` is basePath-stripped, and the login view re-adds the zone prefix
        // when it hands returnTo to the shell — so pass the zone-relative path.
        const returnTo = `${pathname}${window.location.search}`;
        router.replace(returnTo === "/" ? "/login" : `/login?returnTo=${encodeURIComponent(returnTo)}`);
      }),
    [pathname, router],
  );

  return null;
}
