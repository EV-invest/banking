"use client";

// Browser-side platform status hook — the system banner and the admin env badge both
// consume it, so the module-level cache keeps it to ONE `GET /api/platform` per page
// load. Best-effort end to end: a failed fetch resolves to null (consumers render
// nothing) and is not cached; mounted consumers retry on an interval, so a blip at
// first paint doesn't silence the banner for the whole SPA session.

import { useEffect, useState } from "react";

import { apiPath } from "@/shared/config/base-path";
import type { PlatformStatus } from "@/shared/contracts/platform";

const RETRY_MS = 60_000;

let cached: PlatformStatus | null = null;
let inflight: Promise<PlatformStatus | null> | null = null;

function fetchPlatform(): Promise<PlatformStatus | null> {
  // Serving the cache here (not in the hook) closes the render→effect race: a consumer
  // whose effect runs after another consumer's fetch resolved still syncs its state.
  if (cached) return Promise.resolve(cached);
  inflight ??= fetch(apiPath("/api/platform"))
    .then((r) => (r.ok ? (r.json() as Promise<PlatformStatus>) : null))
    .then((p) => {
      cached = p;
      return p;
    })
    .catch(() => null)
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

export function usePlatform(): PlatformStatus | null {
  const [status, setStatus] = useState<PlatformStatus | null>(cached);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const attempt = () => {
      void fetchPlatform().then((p) => {
        if (!active) return;
        if (p) setStatus(p);
        else timer = setTimeout(attempt, RETRY_MS);
      });
    };
    attempt();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, []);
  return status;
}
