"use client";

// The caller's active device sessions, as listed in Settings. Cached so returning to the
// tab that shows them doesn't re-list from the shell every time, and invalidated by the one
// action that changes the list.

import { fetchSessions, revokeSession as revokeSessionRequest } from "@/entities/session/api/sessions-client";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource } from "@/shared/lib/resource";

export const sessionsResource = defineResource({
  name: "auth.sessions",
  fetch: fetchSessions,
  revalidate: 60,
  tags: [TAG.sessions],
});

export async function revokeSession(sessionId: string): Promise<void> {
  await revokeSessionRequest(sessionId);
  sessionsResource.invalidate();
}
