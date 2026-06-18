"use client";

import { useEffect, useState } from "react";

import { cn } from "@/shared/lib/cn";

interface Health {
  ok: boolean;
  backend?: string;
  error?: string;
}

// Calls the BFF /api/health, which calls the hub gRPC HealthService.Check —
// the visible end of the browser → BFF → gRPC smoke path.
export function HealthBadge() {
  const [health, setHealth] = useState<Health | null>(null);

  useEffect(() => {
    fetch("/api/health")
      .then((res) => res.json() as Promise<Health>)
      .then(setHealth)
      .catch(() => setHealth({ ok: false, error: "unreachable" }));
  }, []);

  const ok = health?.ok ?? false;
  return (
    <div className="inline-flex items-center gap-2 rounded-full border border-border px-3 py-1 text-xs">
      <span className={cn("size-2 rounded-full", health == null ? "bg-muted-foreground" : ok ? "bg-main-accent-t2" : "bg-destructive")} />
      <span className="font-mono-tech text-muted-foreground">
        {health == null ? "checking hub…" : ok ? `hub gRPC: ${health.backend}` : `hub gRPC unreachable`}
      </span>
    </div>
  );
}
