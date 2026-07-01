"use client";

// Browser-side session hook: fetches `/api/auth/session` once (the BFF's who-am-I,
// which never returns a token) so client components can read the principal — including
// the platform `role` and the derived `isAdmin` used to gate the admin console nav.
// Server-side authorization is still authoritative; this only shapes the UI.

import { useEffect, useState } from "react";

import type { SessionInfo } from "@/shared/contracts/admin";

export function useSession(): SessionInfo | null {
  const [session, setSession] = useState<SessionInfo | null>(null);
  useEffect(() => {
    let active = true;
    fetch("/api/auth/session")
      .then((r) => r.json() as Promise<SessionInfo>)
      .then((s) => active && setSession(s))
      .catch(() => active && setSession({ authenticated: false }));
    return () => {
      active = false;
    };
  }, []);
  return session;
}
