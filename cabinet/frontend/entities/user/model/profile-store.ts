"use client";

// Small shared profile store: one lazy `GET /api/users` feeds every mounted consumer
// (the sidebar account chip today), and `publishProfile` lets the settings save push
// its PATCH response straight in — the sidebar name refreshes without a refetch.

import { useSyncExternalStore } from "react";

import { fetchProfile } from "@/entities/user/api/profile-client";
import type { UserProfile } from "@/shared/contracts";

let cached: UserProfile | null = null;
// The one-shot GET has completed (resolved OR failed). Lets a consumer tell "still
// loading" apart from "loaded, but no profile", so it can stop showing a loading state
// and fall back to a heuristic (the account chip's email-derived name) instead of
// waiting forever.
let settled = false;
let inflight: Promise<void> | null = null;
const subscribers = new Set<() => void>();

function emit() {
  for (const notify of subscribers) notify();
}

/** Overwrite the cached profile and notify every mounted `useProfile` consumer. */
export function publishProfile(p: UserProfile) {
  cached = p;
  settled = true;
  emit();
}

function ensureFetched() {
  if (cached || inflight) return;
  inflight = fetchProfile()
    .then(publishProfile)
    // Best-effort: consumers fall back to their own heuristics (e.g. the email-derived
    // display name). A failure still "settles" (so consumers stop waiting) but leaves the
    // cache empty to retry on the next mount.
    .catch(() => {
      settled = true;
      emit();
    })
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

/** Whether the one-shot profile fetch has completed (resolved or failed). */
export function useProfileSettled(): boolean {
  return useSyncExternalStore(subscribe, () => settled, () => false);
}
