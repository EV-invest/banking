"use client";

// Browser-side session hook: fetches `/api/auth/session` (the BFF's who-am-I, which
// never returns a token) so client components can read the principal — including the
// platform `role` and the derived `isAdmin` used to gate the admin console nav.
// Server-side authorization is still authoritative; this only shapes the UI.
//
// A module-level cache dedupes the fetch: every consumer on a page (sidebar, account
// chip, admin gate) shares ONE request. Only resolved responses are cached — a fetch
// failure yields the SESSION_UNAVAILABLE sentinel and retries on the next mount, so a
// transient blip never sticks (or bounces anyone to /login).

import { useEffect, useState } from "react";

import { apiPath } from "@/shared/config/base-path";
import type { SessionInfo } from "@/shared/contracts/admin";

/// A fetch/parse failure — same shape as a server-resolved "not signed in", but
/// identity-distinguishable so consumers don't force a re-login on a network blip.
export const SESSION_UNAVAILABLE: SessionInfo = Object.freeze({ authenticated: false });

let cached: SessionInfo | null = null;
let inflight: Promise<SessionInfo> | null = null;

function fetchSession(): Promise<SessionInfo> {
  inflight ??= fetch(apiPath("/api/auth/session"))
    .then((r) => r.json() as Promise<SessionInfo>)
    .then((s) => {
      cached = s;
      return s;
    })
    .catch(() => SESSION_UNAVAILABLE)
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

export function useSession(): SessionInfo | null {
  const [session, setSession] = useState<SessionInfo | null>(cached);
  useEffect(() => {
    if (cached) return;
    let active = true;
    void fetchSession().then((s) => active && setSession(s));
    return () => {
      active = false;
    };
  }, []);
  return session;
}
