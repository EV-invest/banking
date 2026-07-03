"use client";

// Browser-side platform status hook — the system banner and the admin env badge both
// consume it, so the module-level cache keeps it to ONE `GET /api/platform` per page
// load. Best-effort end to end: a failed fetch resolves to null (consumers render
// nothing) and is not cached, so a transient blip retries on the next mount.

import { useEffect, useState } from "react";

import type { PlatformStatus } from "@/shared/contracts/platform";

let cached: PlatformStatus | null = null;
let inflight: Promise<PlatformStatus | null> | null = null;

function fetchPlatform(): Promise<PlatformStatus | null> {
  inflight ??= fetch("/api/platform")
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
    if (cached) return;
    let active = true;
    void fetchPlatform().then((p) => {
      if (active && p) setStatus(p);
    });
    return () => {
      active = false;
    };
  }, []);
  return status;
}
