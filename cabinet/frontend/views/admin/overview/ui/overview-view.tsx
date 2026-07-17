"use client";

import { Activity, Loader2, RefreshCw, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Button, Card, CardContent, Skeleton } from "@evinvest/uikit";

import { fetchOverview, fetchParkedEvents, unparkEvent } from "@/entities/admin/api/admin-client";
import type { AdminOverview, ParkedEvent } from "@/shared/contracts/admin";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { ago } from "@/views/admin/lib/format";
import { AdminHeader, StatusDot } from "@/views/admin/ui/shell";

export function OverviewView() {
  const [overview, setOverview] = useState<AdminOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [parked, setParked] = useState<ParkedEvent[] | null>(null);
  // Best-effort: a money plane that isn't connected renders as a muted hint, not an
  // error banner — the fleet grid above must stay useful without it.
  const [parkedHint, setParkedHint] = useState<string | null>(null);
  const [unparkError, setUnparkError] = useState<string | null>(null);
  const [unparking, setUnparking] = useState<string | null>(null);
  // Rows whose unpark POST succeeded but whose refetch failed — still listed, but they
  // must not offer a second unpark. A successful refetch drops them from the list.
  const [unparked, setUnparked] = useState<ReadonlySet<string>>(new Set());
  const [refetchError, setRefetchError] = useState<string | null>(null);

  // Manual "Run health check" (event handler — may set state synchronously). Refetches
  // BOTH the overview and the parked list so the "Parked rows" KPI and the table agree.
  const load = useCallback(() => {
    setRefreshing(true);
    const overviewDone = fetchOverview()
      .then((o) => {
        setOverview(o);
        setError(null);
      })
      .catch((e: Error) => setError(e.message));
    const parkedDone = fetchParkedEvents()
      .then((l) => {
        setParked(l.events ?? []);
        setParkedHint(null);
        setUnparked(new Set());
        setRefetchError(null);
      })
      .catch((e: Error) => {
        setParked([]);
        setParkedHint(e.message);
      });
    void Promise.allSettled([overviewDone, parkedDone]).then(() => setRefreshing(false));
  }, []);

  // Mount fetch — state is set only in the async callbacks (no synchronous setState in
  // the effect body), so it doesn't trigger cascading renders.
  useEffect(() => {
    let active = true;
    fetchOverview()
      .then((o) => {
        if (!active) return;
        setOverview(o);
        setError(null);
      })
      .catch((e: Error) => active && setError(e.message));
    fetchParkedEvents()
      .then((l) => {
        if (!active) return;
        setParked(l.events ?? []);
        setParkedHint(null);
      })
      .catch((e: Error) => {
        if (!active) return;
        setParked([]);
        setParkedHint(e.message);
      });
    return () => {
      active = false;
    };
  }, []);

  const unpark = async (seq: string) => {
    setUnparking(seq);
    setUnparkError(null);
    setRefetchError(null);
    try {
      const { ok } = await unparkEvent(seq);
      if (!ok) throw new Error("the hub declined the unpark");
    } catch (e) {
      setUnparkError((e as Error).message);
      setUnparking(null);
      return;
    }
    // The POST succeeded — mark the row unparked before the refetch so a refetch
    // failure can't leave an enabled Unpark button on an already-unparked event.
    setUnparked((prev) => new Set(prev).add(seq));
    try {
      // Re-fetch both so the "Parked rows" KPI drops together with the list.
      const [list, o] = await Promise.all([fetchParkedEvents(), fetchOverview()]);
      setParked(list.events ?? []);
      setOverview(o);
      setUnparked(new Set());
    } catch (e) {
      setRefetchError((e as Error).message);
    } finally {
      setUnparking(null);
    }
  };

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

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-5">
        <Kpi label="Services healthy" value={overview ? `${healthy}/${totalServices}` : undefined} tone="text-main-accent-t2" />
        <Kpi label="Parked rows" value={overview?.parked_rows} hint="Money the relay couldn't apply" tone={overview && overview.parked_rows !== "0" ? "text-destructive" : undefined} tip="admin.overview.kpi.parked-rows" />
        <Kpi label="Dispatch backlog" value={overview?.backlog} hint="Undispatched outbox rows" tip="admin.overview.kpi.dispatch-backlog" />
        <Kpi label="Oldest backlog" value={overview ? `${overview.oldest_backlog_age_secs}s` : undefined} hint="Age of the oldest undispatched row" tip="admin.overview.kpi.oldest-backlog" />
        <Kpi
          label="Dead-key signings"
          value={overview?.unseal_failures}
          hint="Signer couldn't unseal a key — funds stranded"
          tone={overview && overview.unseal_failures !== "0" ? "text-destructive" : undefined}
          tip="admin.overview.kpi.dead-key-signings"
        />
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

      <Card>
        <CardContent className="space-y-4 py-5">
          <div>
            <h2 className="text-base font-semibold">Parked events</h2>
            <p className="text-xs text-muted-foreground">Outbox rows the relay couldn&apos;t apply — fix the cause, then unpark to re-drive</p>
          </div>
          {unparkError && (
            <p className="flex items-center gap-2 text-xs text-destructive">
              <TriangleAlert className="size-3.5" /> {unparkError}
            </p>
          )}
          {refetchError && (
            <p className="flex items-center gap-2 text-xs text-main-accent-t3">
              <TriangleAlert className="size-3.5" /> Unparked, but refreshing the list failed: {refetchError} — run a health check to resync.
            </p>
          )}
          {!parked ? (
            <Skeleton className="h-16 w-full" />
          ) : parkedHint ? (
            <p className="text-sm text-muted-foreground">{parkedHint}</p>
          ) : parked.length === 0 ? (
            <p className="text-sm text-muted-foreground">No parked events — the relay is clean.</p>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                  <th className="py-2 font-medium">Seq</th>
                  <th className="py-2 font-medium">Event</th>
                  <th className="py-2 font-medium">
                    <span className="flex items-center gap-1.5">
                      Reason
                      <TipAnchor anchor="admin.overview.parked.reason" />
                    </span>
                  </th>
                  <th className="py-2 font-medium">Parked</th>
                  <th className="py-2 font-medium" />
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                {parked.map((e) => (
                  <tr key={e.seq}>
                    <td className="py-2.5 font-mono-tech text-xs text-muted-foreground">{e.seq}</td>
                    <td className="py-2.5">
                      <p className="font-medium">{e.kind}</p>
                      <p className="font-mono-tech text-xs text-muted-foreground">
                        {e.aggregate} · {e.aggregate_id}
                      </p>
                    </td>
                    <td className="py-2.5 text-muted-foreground">
                      <div className="max-w-[280px] truncate" title={e.reason}>
                        {e.reason || "—"}
                      </div>
                    </td>
                    <td className="whitespace-nowrap py-2.5 text-muted-foreground">{ago(e.parked_at)}</td>
                    <td className="py-2.5 text-right">
                      <div className="flex items-center justify-end gap-2">
                        {e.compensated && (
                          <span className="flex items-center gap-1.5 rounded-full bg-foreground/[0.06] px-2 py-0.5 text-xs font-medium text-main-mist">
                            compensated
                            <TipAnchor anchor="admin.overview.parked.compensated" />
                          </span>
                        )}
                        {unparked.has(e.seq) && <span className="rounded-full bg-main-accent-t2/15 px-2 py-0.5 text-xs font-medium text-main-accent-t2">unparked</span>}
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          disabled={e.compensated || unparked.has(e.seq) || unparking !== null}
                          onClick={() => void unpark(e.seq)}
                        >
                          {unparking === e.seq ? <Loader2 className="size-3.5 animate-spin" /> : null}
                          Unpark
                        </Button>
                        <TipAnchor anchor="admin.overview.parked.unpark" />
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function Kpi({ label, value, hint, tone, tip }: { label: string; value: string | undefined; hint?: string; tone?: string; tip?: TipKey }) {
  return (
    <Card>
      <CardContent className="space-y-1 py-5">
        <div className="flex items-center gap-1.5">
          <p className="text-xs uppercase tracking-wide text-muted-foreground">{label}</p>
          {tip && <TipAnchor anchor={tip} />}
        </div>
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
