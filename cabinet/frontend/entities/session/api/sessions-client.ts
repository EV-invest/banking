// Browser → shell sessions client (site-root /api/auth — auth is shell-owned, not this
// zone's, hence `scope: "shell"`: no basePath). Lists the user's active device sessions
// and revokes one by id. Transport, CSRF and session handling belong to
// `@/shared/lib/api-client`. Identity is the server-side refresh token (held by the
// shell's auth surface) — never exposed here.

import { requestJson } from "@/shared/lib/api-client";
import type { Session, SessionList } from "@/shared/contracts";

export async function fetchSessions(): Promise<Session[]> {
  const data = await requestJson<SessionList>("/api/auth/sessions", { scope: "shell" });
  return data.sessions ?? [];
}

export async function revokeSession(sessionId: string): Promise<void> {
  await requestJson("/api/auth/sessions", { method: "DELETE", body: { session_id: sessionId }, scope: "shell" });
}
