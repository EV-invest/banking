"use client";

import { Loader2, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Button, Card, CardContent, Input, Skeleton } from "@evinvest/uikit";

import { failRedemption, fetchAllocations, fetchRedemptionQueue, postValuation, settleRedemption } from "@/entities/admin/api/admin-client";
import { apiPath } from "@/shared/config/base-path";
import type { Allocation, FundNav, RedemptionQueueItem } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { ago, formatNav, formatUsd } from "@/views/admin/lib/format";
import { AdminHeader, Toggle } from "@/views/admin/ui/shell";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

// Read the current fund NAV (units_outstanding drives the derived-NAV preview and the
// queue's est-cash), via the existing self-service money route.
async function fetchFundNav(service: string): Promise<FundNav> {
  const res = await fetch(apiPath(`/api/funds/nav?service=${encodeURIComponent(service)}`), { headers: { accept: "application/json" } });
  const data = (await res.json().catch(() => ({}))) as FundNav & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `nav unavailable (${res.status})`);
  return data;
}

export function ValuationView() {
  // The fund is PICKED from the registry, never typed: the hub refuses a valuation for
  // an unregistered service, so a free-text field could only ever produce a NOT_FOUND.
  // Drafts and closed products are listed too — a closed fund still gets marked so its
  // queued redemptions price correctly.
  const [allocations, setAllocations] = useState<Allocation[] | null>(null);
  const [service, setService] = useState("");
  const [aum, setAum] = useState("");
  const [override, setOverride] = useState(false);
  const [nav, setNav] = useState<FundNav | null>(null);
  const [posting, setPosting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [queue, setQueue] = useState<RedemptionQueueItem[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const loadQueue = useCallback(() => {
    fetchRedemptionQueue()
      .then((q) => setQueue(q.items ?? []))
      .catch((e: Error) => setError(e.message));
  }, []);

  const loadNav = useCallback((svc: string) => {
    if (!svc) {
      setNav(null);
      return;
    }
    fetchFundNav(svc)
      .then(setNav)
      .catch(() => setNav(null));
  }, []);

  useEffect(() => {
    loadQueue();
    fetchAllocations()
      .then((list) => {
        const rows = list.allocations ?? [];
        setAllocations(rows);
        // Default to the first product that can actually take a mark, so the common case
        // needs no interaction; fall back to the first row when none is open yet.
        const initial = rows.find((a) => a.state === "open") ?? rows[0];
        if (initial) {
          setService(initial.service);
          loadNav(initial.service);
        }
      })
      .catch((e: Error) => setError(e.message));
  }, [loadQueue, loadNav]);

  // Live derived NAV preview = entered AUM / current units.
  const units = Number(nav?.units_outstanding ?? "0");
  const aumNum = Number(aum || "0");
  const derivedNav = units > 0 && aumNum > 0 ? aumNum / units : null;
  const currentNav = derivedNav ?? Number(nav?.nav ?? "0");

  const post = async () => {
    setPosting(true);
    setError(null);
    try {
      const posted = await postValuation({ service, aum, override });
      setNav(posted);
      setAum("");
      loadQueue();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setPosting(false);
    }
  };

  const act = async (fn: (id: string) => Promise<unknown>, id: string) => {
    setBusy(id);
    setError(null);
    try {
      await fn(id);
      loadQueue();
      // Settle burns units, so the derived-NAV preview and the queue's est-cash go
      // stale on the load-time units_outstanding — refresh the mark alongside the queue.
      loadNav(service);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-8 px-8 pb-10 pt-6">
      <AdminHeader eyebrow="Administer" title="Valuation & redemptions" subtitle="Post fund NAV and clear the redemption queue" />

      {error && (
        <p className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4" /> {error}
        </p>
      )}

      <section className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">Post valuation</p>
        <Card>
          <CardContent className="space-y-5 py-6">
            <div className="grid gap-4 md:grid-cols-3">
              <label className="block space-y-1.5">
                <span className="text-sm text-muted-foreground">Fund (service)</span>
                <select
                  value={service}
                  onChange={(e) => {
                    setService(e.target.value);
                    loadNav(e.target.value);
                  }}
                  disabled={!allocations || allocations.length === 0}
                  className="h-9 w-full rounded-md border border-border bg-main-surface px-2 text-sm outline-none focus:border-main-accent-t1 disabled:opacity-60"
                >
                  {!allocations ? (
                    <option value="">Loading…</option>
                  ) : allocations.length === 0 ? (
                    <option value="">No allocations registered</option>
                  ) : (
                    allocations.map((a) => (
                      <option key={a.service} value={a.service}>
                        {a.title} ({a.service}){a.state === "open" ? "" : ` — ${a.state}`}
                      </option>
                    ))
                  )}
                </select>
              </label>
              <label className="block space-y-1.5">
                <span className="flex items-center gap-1.5 text-sm text-muted-foreground">
                  AUM (USDT)
                  <TipAnchor anchor="admin.valuation.post.aum" />
                </span>
                <Input value={aum} onChange={(e) => setAum(e.target.value)} inputMode="decimal" placeholder="0.00" />
              </label>
              <div className="space-y-1.5">
                <span className="flex items-center gap-1.5 text-sm text-muted-foreground">
                  Derived NAV / share
                  <TipAnchor anchor="admin.valuation.post.derived-nav" />
                </span>
                <div className="flex h-9 items-center rounded-md border border-main-accent-t1/40 bg-main-accent-t1/10 px-3 text-sm">
                  <span className="font-semibold text-main-accent-t1 tabular-nums">{derivedNav ? formatNav(derivedNav) : "—"}</span>
                  {units > 0 && <span className="ml-2 text-xs tabular-nums text-muted-foreground">= AUM / {units.toLocaleString("en-US")} units</span>}
                </div>
              </div>
            </div>

            <div className="rounded-lg border border-main-accent-t3/30 bg-main-accent-t3/5 px-4 py-2.5 text-sm text-main-accent-t3">
              <TriangleAlert className="mr-2 inline size-4" />
              NAV-move guard — a post is rejected if NAV moves more than 50% from the last mark, unless override is on.
            </div>

            <div className="flex items-center gap-4">
              <div className="flex items-center gap-2">
                <Toggle on={override} onChange={setOverride} label="Override guard" />
                <div className="text-sm">
                  <p className="flex items-center gap-1.5">
                    Override guard
                    <TipAnchor anchor="admin.valuation.post.override" />
                  </p>
                  <p className="text-xs text-muted-foreground">Allow a &gt;50% NAV move</p>
                </div>
              </div>
              <Button type="button" className={cn("ml-auto", TEAL_CTA)} disabled={posting || !aum || !service} onClick={post}>
                {posting ? <Loader2 className="size-4 animate-spin" /> : null}
                Post valuation
              </Button>
            </div>
          </CardContent>
        </Card>
      </section>

      <section className="space-y-3">
        <p className="flex items-center gap-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
          Redemption queue
          {/* The count pill lands on the same step as the label it trails, so its fill and
              accent colour — not a smaller size — are what set it apart. */}
          {queue && (
            <span className="rounded-full bg-main-accent-t3/15 px-2 py-0.5 text-xs font-semibold text-main-accent-t3">{queue.length} queued</span>
          )}
        </p>
        <Card>
          <CardContent className="p-0">
            {!queue ? (
              <div className="p-6">
                <Skeleton className="h-32 w-full" />
              </div>
            ) : queue.length === 0 ? (
              <p className="p-8 text-center text-sm text-muted-foreground">The redemption queue is empty.</p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                    <th className="px-5 py-3 font-medium">User</th>
                    <th className="px-5 py-3 font-medium">Units</th>
                    <th className="px-5 py-3 font-medium">
                      <span className="flex items-center gap-1.5">
                        Est. cash
                        <TipAnchor anchor="admin.valuation.queue.est-cash" />
                      </span>
                    </th>
                    <th className="px-5 py-3 font-medium">Age</th>
                    <th className="px-5 py-3 text-right font-medium">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {queue.map((item) => {
                    const est = currentNav > 0 ? Number(item.units) * currentNav : null;
                    return (
                      <tr key={item.redemption_id}>
                        <td className="px-5 py-3">
                          <p className="font-medium">{item.email || item.user_id.slice(0, 8)}</p>
                          <p className="font-mono-tech text-xs text-muted-foreground">{item.service}</p>
                        </td>
                        <td className="px-5 py-3 tabular-nums">{Number(item.units).toLocaleString("en-US")}</td>
                        <td className="px-5 py-3 tabular-nums text-muted-foreground">{est ? `≈ ${formatUsd(est)}` : "—"}</td>
                        <td className="px-5 py-3 text-muted-foreground">{ago(item.created_at)}</td>
                        <td className="px-5 py-3">
                          <div className="flex justify-end gap-2">
                            <span className="inline-flex items-center gap-1">
                              <Button type="button" variant="outline" size="sm" disabled={busy === item.redemption_id} onClick={() => act(settleRedemption, item.redemption_id)}>
                                Settle
                              </Button>
                              <TipAnchor anchor="admin.valuation.queue.settle" />
                            </span>
                            <span className="inline-flex items-center gap-1">
                              <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                className="border-destructive/40 text-destructive hover:bg-destructive/10"
                                disabled={busy === item.redemption_id}
                                onClick={() => act(failRedemption, item.redemption_id)}
                              >
                                Fail
                              </Button>
                              <TipAnchor anchor="admin.valuation.queue.fail" />
                            </span>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </CardContent>
        </Card>
        <p className="max-w-3xl text-xs text-muted-foreground">Settle pays at settle-time NAV once the fund claim is liquid; if the rail is short the payout queues until treasury tops up. Fail voids the request and refunds the units.</p>
      </section>
    </div>
  );
}
