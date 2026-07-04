"use client";

// Small shared profile store: one lazy `GET /api/users` feeds every mounted consumer
// (the sidebar account chip today), and `publishProfile` lets the settings save push
// its PATCH response straight in — the sidebar name refreshes without a refetch.

import { useSyncExternalStore } from "react";

import { fetchProfile } from "@/entities/user/api/profile-client";
import type { UserProfile } from "@/shared/contracts";

let cached: UserProfile | null = null;
let inflight: Promise<void> | null = null;
const subscribers = new Set<() => void>();

/** Overwrite the cached profile and notify every mounted `useProfile` consumer. */
export function publishProfile(p: UserProfile) {
  cached = p;
  for (const notify of subscribers) notify();
}

function ensureFetched() {
  if (cached || inflight) return;
  inflight = fetchProfile()
    .then(publishProfile)
    // Best-effort: consumers fall back to their own heuristics (e.g. the email-derived
    // display name); an uncached failure retries on the next mount.
    .catch(() => undefined)
    .finally(() => {
      inflight = null;
    });
}

function subscribe(onStoreChange: () => void): () => void {
  subscribers.add(onStoreChange);
  ensureFetched();
  return () => {
    subscribers.delete(onStoreChange);
  };
}

export function useProfile(): UserProfile | null {
  return useSyncExternalStore(subscribe, () => cached, () => null);
}
