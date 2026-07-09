// Browser → shell sessions client (site-root /api/auth — auth is shell-owned, not this
// zone's). Lists the user's active device sessions and revokes one by id. The DELETE
// carries the CSRF double-submit header. Identity is the server-side refresh token
// (held by the shell's auth surface) — never exposed here.

import { csrfHeader } from "@/shared/lib/csrf-client";
import type { Session, SessionList } from "@/shared/contracts";

export async function fetchSessions(): Promise<Session[]> {
  const res = await fetch("/api/auth/sessions", { headers: { accept: "application/json" } });
  const data = (await res.json().catch(() => ({}))) as SessionList & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `request failed (${res.status})`);
  return data.sessions ?? [];
}

export async function revokeSession(sessionId: string): Promise<void> {
  const res = await fetch("/api/auth/sessions", {
    method: "DELETE",
    headers: { "content-type": "application/json", ...csrfHeader() },
    body: JSON.stringify({ session_id: sessionId }),
  });
  if (!res.ok) {
    const data = (await res.json().catch(() => ({}))) as { error?: string };
    throw new Error(data.error ?? `revoke failed (${res.status})`);
  }
}
