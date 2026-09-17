"use client";

// The session list at the hub and the two ways to shorten it. Its own read, refreshed by
// the revoke rather than by a manual reload.

import { useT } from "@evinvest/i18n/react";
import { useState } from "react";

import { revokeSession, sessionsResource } from "@/entities/session/model/session-resource";
import { errorMessage } from "@/shared/lib/api-client";
import { useResource } from "@/shared/lib/resource";

export function useSessions() {
  const t = useT();
  const [revokeError, setRevokeError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const list = useResource(sessionsResource);
  const sessions = list.data;
  const error = revokeError ?? (sessions || !list.error ? null : errorMessage(list.error, t));

  async function revoke(id: string) {
    setBusy(true);
    setRevokeError(null);
    try {
      // The revoke invalidates the session list, so it refreshes itself.
      await revokeSession(id);
    } catch (e) {
      setRevokeError(errorMessage(e, t));
    } finally {
      setBusy(false);
    }
  }
  async function revokeOthers() {
    const others = (sessions ?? []).filter((s) => !s.current && s.id);
    if (!others.length) return;
    setBusy(true);
    setRevokeError(null);
    try {
      for (const s of others) await revokeSession(s.id!);
    } catch (e) {
      setRevokeError(errorMessage(e, t));
    } finally {
      setBusy(false);
    }
  }

  return { sessions, error, busy, revoke, revokeOthers };
}
