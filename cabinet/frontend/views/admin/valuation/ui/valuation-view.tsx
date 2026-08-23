"use client";

import { Loader2, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { Button, Card, CardContent, Input, Select, SelectContent, SelectItem, SelectTrigger, Skeleton } from "@evinvest/uikit";

import { failRedemption, postValuation, setAllocationUnitCap, settleRedemption } from "@/entities/admin/api/admin-client";
import { adminAllocationsResource, redemptionQueueResource } from "@/entities/admin/model/admin-resource";
import { fundNavResource } from "@/entities/fund/model/fund-resource";
import type { Allocation } from "@/shared/contracts/admin";
import { TAG } from "@/shared/lib/cache-tags";
import { cn } from "@/shared/lib/cn";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { Settled, StaggerItem } from "@/shared/ui/motion";
import { TipAnchor } from "@/shared/tips";
import { ago, compactUnits, formatNav, formatUnits, formatUsd, fractionOfCap, toBaseUnits } from "@/views/admin/lib/format";
import { AdminHeader, AdminScreen, Toggle } from "@/views/admin/ui/shell";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

// "EV Trading (trading)", with the state trailing when it is not the plain open case.
function allocationLabel(a: Allocation): string {
  return `${a.title} (${a.service})${a.state === "open" ? "" : ` — ${a.state}`}`;
}

export function ValuationView() {
  // The fund is PICKED from the registry, never typed: the hub refuses a valuation for
  // an unregistered service, so a free-text field could only ever produce a NOT_FOUND.
  // Drafts and closed products are listed too — a closed fund still gets marked so its
  // queued redemptions price correctly.
  const [service, setService] = useState("");
  const [aum, setAum] = useState("");
  const [override, setOverride] = useState(false);
  const [posting, setPosting] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const registry = useResource(adminAllocationsResource);
  const queueRead = useResource(redemptionQueueResource);
  // The same per-fund NAV entry the investor screens read, so a mark posted here lands on
  // the product page and the fund cards without either of them re-fetching.
  const navRead = useResource(fundNavResource, service);

  const allocations = registry.data ? (registry.data.allocations ?? []) : null;
  const queue = queueRead.data ? (queueRead.data.items ?? []) : null;
  const nav = navRead.data ?? null;
  const error = actionError ?? (allocations ? null : (registry.error?.message ?? null)) ?? (queue ? null : (queueRead.error?.message ?? null));

  // Default to the first product that can actually take a mark, so the common case needs no
  // interaction; fall back to the first row when none is open yet. Chosen during render, so
  // a cached registry means the form arrives already pointed at a fund.
  const [picked, setPicked] = useState(false);
  if (!picked && allocations) {
    setPicked(true);
    const initial = allocations.find((a) => a.state === "open") ?? allocations[0];
    if (initial) setService(initial.service);
  }

  // Live derived NAV preview = entered AUM / current units. NAV is *derived*, never
  // entered — hence the read-only box below.
  const units = Number(nav?.units_outstanding ?? "0");
  const aumNum = Number(aum || "0");
  const derivedNav = units > 0 && aumNum > 0 ? aumNum / units : null;
  const currentNav = derivedNav ?? Number(nav?.nav ?? "0");
  // A fund nobody has subscribed to has no units, so AUM / units is undefined and the
  // hub rejects the post outright (`nav undefined: no units outstanding`). Say so here
  // instead of letting the operator fill the form and meet a raw domain error — but gate
  // only the POST. Writing the figure down is not what is impossible, so the AUM field
  // stays usable.
  const noUnits = nav !== null && units === 0;
  const selected = allocations?.find((a) => a.service === service) ?? null;

  const post = async () => {
    setPosting(true);
    setActionError(null);
    try {
      // The POST answers with the new mark, so it is published straight in rather than
      // re-read — and every investor surface showing this fund's price follows.
      fundNavResource.publish(await postValuation({ service, aum, override }), service);
      setAum("");
      await queueRead.refresh();
    } catch (e) {
      setActionError((e as Error).message);
    } finally {
      setPosting(false);
    }
  };

  const act = async (fn: (id: string) => Promise<unknown>, id: string) => {
    setBusy(id);
    setActionError(null);
    try {
      await fn(id);
      // Settling burns units, so the derived-NAV preview and the queue's est-cash go stale
      // on the units_outstanding this screen loaded with — and so do the investor's own
      // position and redemption lists. One tag sweep covers all of them.
      revalidateTag(TAG.nav, TAG.positions, TAG.redemptions, TAG.operations);
      await Promise.all([queueRead.refresh(), navRead.refresh()]);
    } catch (e) {
      setActionError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  return (
    <AdminScreen className="space-y-8">
      <AdminHeader eyebrow="Administer" title="Valuation & redemptions" subtitle="Post fund NAV and clear the redemption queue" />

      {error && (
        <StaggerItem as="p" className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4" /> {error}
        </StaggerItem>
      )}

      <StaggerItem as="section" className="space-y-3" id="post-valuation">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">Post valuation</p>
        <Card>
          <CardContent className="space-y-5 py-6">
            <div className="grid gap-4 md:grid-cols-3">
              <div className="flex flex-col gap-1.5">
                <span className="text-sm text-muted-foreground">Fund (service)</span>
                <Select
                  value={service || undefined}
                  onValueChange={setService}
                >
                  {/* `disabled` lives on the trigger — `Select` itself is a pure state
                      container and takes no such prop. */}
                  <SelectTrigger className="w-full border-border bg-main-surface" disabled={!allocations || allocations.length === 0}>
                    {/* Not `SelectValue`: the uikit's renders the raw stored value, so the
                        trigger would read the bare slug instead of the product's title. */}
                    <span className={cn("truncate", !selected && "text-muted-foreground")}>
                      {selected ? allocationLabel(selected) : !allocations ? "Loading…" : "No allocations registered"}
                    </span>
                  </SelectTrigger>
                  <SelectContent>
                    {(allocations ?? []).map((a) => (
                      <SelectItem key={a.service} value={a.service}>
                        {allocationLabel(a)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <label className="flex flex-col gap-1.5">
                <span className="flex items-center gap-1.5 text-sm text-muted-foreground">
                  AUM (USDT)
                  <TipAnchor anchor="admin.valuation.post.aum" />
                </span>
                <Input value={aum} onChange={(e) => setAum(e.target.value)} inputMode="decimal" placeholder="0.00" className="w-full" />
              </label>
              <div className="flex flex-col gap-1.5">
                <span className="flex items-center gap-1.5 text-sm text-muted-foreground">
                  Derived NAV / share
                  <TipAnchor anchor="admin.valuation.post.derived-nav" />
                </span>
                {/* Read-only on purpose: NAV is derived (AUM / units read live from the
                    ledger), never posted directly — an editable field here would imply
                    an operator can set a price. */}
                <div className="flex h-9 items-center rounded-md border border-main-accent-t1/40 bg-main-accent-t1/10 px-3 text-sm" aria-readonly="true">
                  <span className="font-semibold text-main-accent-t1 tabular-nums">{derivedNav ? formatNav(derivedNav) : "—"}</span>
                  {units > 0 && <span className="ml-2 text-xs tabular-nums text-muted-foreground">= AUM / {units.toLocaleString("en-US")} units</span>}
                </div>
              </div>
            </div>

            {noUnits ? (
              <div className="rounded-lg border border-border bg-foreground/5 px-4 py-2.5 text-sm text-muted-foreground">
                <TriangleAlert className="mr-2 inline size-4" />
                No units outstanding — NAV is <span className="font-semibold text-foreground">AUM / units</span>, so it is undefined until someone subscribes. The fund prices at the seed
                NAV 1.0 until the first subscription; there is nothing to mark yet.
              </div>
            ) : (
              <div className="rounded-lg border border-main-accent-t3/30 bg-main-accent-t3/5 px-4 py-2.5 text-sm text-main-accent-t3">
                <TriangleAlert className="mr-2 inline size-4" />
                NAV-move guard — a post is rejected if NAV moves more than 50% from the last mark, unless override is on.
              </div>
            )}

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
              <Button type="button" className={cn("ml-auto", TEAL_CTA)} disabled={posting || !aum || !service || noUnits} onClick={post}>
                {posting ? <Loader2 className="size-4 animate-spin" /> : null}
                Post valuation
              </Button>
            </div>
          </CardContent>
        </Card>
      </StaggerItem>

      <StaggerItem as="section" className="space-y-3">
        <p className="flex items-center gap-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
          Unit supply
          <TipAnchor anchor="admin.valuation.post.derived-nav" />
        </p>
        <SupplyCapCard
          allocation={selected}
          nav={nav}
          onSaved={(updated) => {
            // The cap answer carries the whole allocation, so it is written straight into
            // the registry entry — and the cap gates money, so the investor-facing catalog
            // and this fund's mark are told too.
            const rows = registry.data?.allocations ?? [];
            adminAllocationsResource.publish({ ...registry.data, allocations: rows.map((a) => (a.service === updated.service ? updated : a)) });
            revalidateTag(TAG.catalog);
            void navRead.refresh();
          }}
          onError={setActionError}
        />
      </StaggerItem>

      <StaggerItem as="section" className="space-y-3">
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
            <Settled
              loading={!queue}
              skeleton={
                <div className="p-6">
                  <Skeleton className="h-32 w-full" />
                </div>
              }
            >
              {!queue ? null : queue.length === 0 ? (
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
            </Settled>
          </CardContent>
        </Card>
        <p className="max-w-3xl text-xs text-muted-foreground">Settle pays at settle-time NAV once the fund claim is liquid; if the rail is short the payout queues until treasury tops up. Fail voids the request and refunds the units.</p>
      </StaggerItem>
    </AdminScreen>
  );
}

/// How many units this product may ever issue. Lives on the allocation, not on a
/// valuation mark: a mark is an immutable historical price, while the cap is a policy an
/// operator revises — storing it on the mark would make "change the cap" mean "post a
/// price", and tangle the two histories together.
function SupplyCapCard({
  allocation,
  nav,
  onSaved,
  onError,
}: {
  allocation: Allocation | null;
  nav: { units_outstanding?: string; remaining_capacity?: string } | null;
  onSaved: (updated: Allocation) => void;
  onError: (message: string | null) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const cap = allocation?.unit_cap ?? "";
  const issued = nav?.units_outstanding ?? "0";
  // Uncontrolled until the operator types: the field shows the stored cap, and switching
  // funds re-reads it rather than carrying the previous product's number across.
  const value = draft ?? cap;
  const parsed = toBaseUnits(value);
  const invalid = value.trim() !== "" && parsed <= 0n;
  const changed = value.trim() !== "" && parsed !== toBaseUnits(cap);
  const fraction = fractionOfCap(issued, cap);
  // A cap narrowed below what is already out is legal — it stops issuance without
  // touching a single minted unit — but it is worth saying out loud before saving.
  const belowIssued = changed && !invalid && parsed < toBaseUnits(issued);
  const nearCap = fraction >= 0.9;

  const save = async () => {
    if (!allocation) return;
    setSaving(true);
    onError(null);
    try {
      onSaved(await setAllocationUnitCap(allocation.service, value.trim()));
      setDraft(null);
    } catch (e) {
      onError((e as Error).message);
    } finally {
      setSaving(false);
    }
  };

  if (!allocation) {
    return (
      <Card>
        <CardContent className="py-6 text-sm text-muted-foreground">Pick a fund above to see and resize its authorised unit supply.</CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardContent className="space-y-5 py-6">
        <div className="space-y-2">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <span className="text-sm text-muted-foreground">
              Units issued in <span className="font-mono-tech text-foreground">{allocation.service}</span>
            </span>
            <span className={cn("text-sm font-semibold tabular-nums", nearCap ? "text-main-accent-t3" : "text-foreground")}>
              {compactUnits(issued)} / {compactUnits(cap)} units
            </span>
          </div>
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-border">
            {/* Proportional, no minimum sliver — see `SupplyBar`. The exact issued figure
                sits directly above it. */}
            <div className={cn("h-full rounded-full", nearCap ? "bg-main-accent-t3" : "bg-main-accent-t1")} style={{ width: `${fraction * 100}%` }} />
          </div>
          <p className="text-xs text-muted-foreground">
            {nav ? `${formatUnits(nav.remaining_capacity)} units still issuable` : "Loading supply…"} — a subscription that would mint past the cap is refused.
          </p>
        </div>

        <div className="flex flex-wrap items-end gap-3">
          <label className="flex w-56 flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">Cap (units)</span>
            <Input value={value} onChange={(e) => setDraft(e.target.value)} inputMode="decimal" placeholder="100000000" className="w-full" />
          </label>
          <Button type="button" className={cn(TEAL_CTA)} disabled={saving || invalid || !changed} onClick={save}>
            {saving ? <Loader2 className="size-4 animate-spin" /> : null}
            Save cap
          </Button>
          {draft !== null && (
            <Button type="button" variant="outline" onClick={() => setDraft(null)}>
              Reset
            </Button>
          )}
          <p className={cn("min-w-48 flex-1 text-xs", invalid ? "text-destructive" : belowIssued ? "text-main-accent-t3" : "text-muted-foreground")}>
            {invalid
              ? "Must be greater than zero — to stop new money entirely, close the allocation instead."
              : belowIssued
                ? `Below the ${compactUnits(issued)} units already issued: this stops further issuance. Nothing minted is affected and redemptions keep working.`
                : "Takes effect on the next subscription. Redemptions are never affected."}
          </p>
        </div>
      </CardContent>
    </Card>
  );
}
