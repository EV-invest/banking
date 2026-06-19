"use client";

import { useEffect, useState } from "react";

import { csrfHeader } from "@/features/auth/lib/csrf-client";

interface SessionResponse {
  authenticated: boolean;
  user?: { email: string };
}

// Header session control: reflects /api/auth/session and offers Sign in / Sign out.
// The browser only ever sees the opaque session cookie; tokens stay in the BFF.
export function UserMenu() {
  const [session, setSession] = useState<SessionResponse | null>(null);

  useEffect(() => {
    fetch("/api/auth/session")
      .then((res) => res.json() as Promise<SessionResponse>)
      .then(setSession)
      .catch(() => setSession({ authenticated: false }));
  }, []);

  async function signOut() {
    await fetch("/api/auth/logout", { method: "POST", headers: csrfHeader() });
    window.location.href = "/loggedout";
  }

  if (session == null) {
    return <span className="text-sm text-muted-foreground">…</span>;
  }
  if (!session.authenticated) {
    // Full navigation to the BFF route that 302-redirects to Google — not a page,
    // so `<Link>` (client routing) does not apply.
    return (
      // eslint-disable-next-line @next/next/no-html-link-for-pages
      <a href="/api/auth/login" className="hover:text-foreground">
        Sign in
      </a>
    );
  }
  return (
    <div className="flex items-center gap-3">
      <span className="text-muted-foreground">{session.user?.email}</span>
      <button type="button" onClick={signOut} className="rounded-md border border-border px-2.5 py-1 hover:text-foreground">
        Sign out
      </button>
    </div>
  );
}
