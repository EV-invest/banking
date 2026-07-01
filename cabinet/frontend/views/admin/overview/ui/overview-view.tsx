"use client";

import { Activity, RefreshCw, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Button, Card, CardContent, Skeleton } from "@evinvest/uikit";

import { fetchOverview } from "@/entities/admin/api/admin-client";
import type { AdminOverview } from "@/shared/contracts/admin";
import { AdminHeader, StatusDot } from "@/views/admin/ui/shell";

export function OverviewView() {
  const [overview, setOverview] = useState<AdminOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const load = useCallback(() => {
    setRefreshing(true);
    fetchOverview()
      .then((o) => {
        setOverview(o);
        setError(null);
      })
      .catch((e: Error) => setError(e.message))
      .finally(() => setRefreshing(false));
  }, []);

  useEffect(load, [load]);

  const healthy = overview?.services.filter((s) => s.status === "healthy").length ?? 0;
  const totalServices = overview?.services.length ?? 0;

  return (
    <div className="space-y-8 px-8 pb-10 pt-6">
      <AdminHeader
        eyebrow="Administer"
        title="Overview"
        subtitle="Central service and microservices — health and throughput"
        action={
          <Button type="button" variant="outline" size="sm" disabled={refreshing} onClick={load}>
            <RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} /> Run health check
          </Button>
        }
      />

      {error && (
        <p className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4" /> {error}
        </p>
      )}

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Kpi label="Services healthy" value={overview ? `${healthy}/${totalServices}` : undefined} tone="text-main-accent-t2" />
        <Kpi label="Parked rows" value={overview?.parked_rows} hint="Money the relay couldn't apply" tone={overview && overview.parked_rows !== "0" ? "text-destructive" : undefined} />
        <Kpi label="Dispatch backlog" value={overview?.backlog} hint="Undispatched outbox rows" />
        <Kpi label="Oldest backlog" value={overview ? `${overview.oldest_backlog_age_secs}s` : undefined} hint="Age of the oldest undispatched row" />
      </div>

      <div className="grid gap-6 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardContent className="space-y-4 py-5">
            <div>
              <h2 className="text-base font-semibold">Fleet health</h2>
              <p className="text-xs text-muted-foreground">Central hub · datastores · microservices</p>
            </div>
            {!overview ? (
              <Skeleton className="h-48 w-full" />
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                    <th className="py-2 font-medium">Service</th>
                    <th className="py-2 font-medium">Kind</th>
                    <th className="py-2 font-medium">Status</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {overview.services.map((s) => (
                    <tr key={s.name}>
                      <td className="py-2.5 font-medium">{s.name}</td>
                      <td className="py-2.5 capitalize text-muted-foreground">{s.kind}</td>
                      <td className="py-2.5">
                        <StatusDot status={s.status} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardContent className="space-y-3 py-5">
            <div className="flex items-center gap-2">
              <Activity className="size-4 text-main-accent-t1" />
              <h2 className="text-base font-semibold">Errors & analytics</h2>
            </div>
            <ObsPanel label="Sentry" hint="Unresolved issues across the fleet surface here once SENTRY_DSN is configured." />
            <ObsPanel label="PostHog" hint="Active investors, sessions and top events surface here once POSTHOG_KEY is configured." />
            <ObsPanel label="Event stream" hint="A live outbox/event feed renders here when streaming is enabled." />
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function Kpi({ label, value, hint, tone }: { label: string; value: string | undefined; hint?: string; tone?: string }) {
  return (
    <Card>
      <CardContent className="space-y-1 py-5">
        <p className="text-xs uppercase tracking-wide text-muted-foreground">{label}</p>
        {value === undefined ? <Skeleton className="mt-1 h-8 w-20" /> : <p className={`text-3xl font-semibold tabular-nums ${tone ?? ""}`}>{value}</p>}
        {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
      </CardContent>
    </Card>
  );
}

function ObsPanel({ label, hint }: { label: string; hint: string }) {
  return (
    <div className="rounded-lg border border-dashed border-border p-3">
      <p className="text-xs font-semibold text-main-mist">{label}</p>
      <p className="text-xs text-muted-foreground">{hint}</p>
    </div>
  );
}
