"use client";

import { Loader2, Plus, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Button, Card, CardContent, Input, Skeleton } from "@evinvest/uikit";

import { fetchAllocations, registerAllocation, setAllocationState, updateAllocation } from "@/entities/admin/api/admin-client";
import type { Allocation, AllocationState } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { Settled } from "@/shared/ui/motion";
import { compactUnits } from "@/views/admin/lib/format";
import { AdminHeader } from "@/views/admin/ui/shell";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

// The registry's own vocabulary, rendered. `closed` is amber rather than destructive:
// it stops new subscriptions but investors can still redeem out of it, so it is a
// wind-down, not a failure.
const STATE_TONE: Record<AllocationState, string> = {
  draft: "border-border text-muted-foreground",
  open: "border-main-accent-t2/40 bg-main-accent-t2/10 text-main-accent-t2",
  closed: "border-main-accent-t3/40 bg-main-accent-t3/10 text-main-accent-t3",
};

const STATE_HINT: Record<AllocationState, string> = {
  draft: "Registered — accepts no money until opened",
  open: "Accepting subscriptions and redemptions",
  closed: "Closed to new money; redemptions still settle",
};

export function AllocationsView() {
  const [rows, setRows] = useState<Allocation[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);

  const load = useCallback(() => {
    fetchAllocations()
      .then((list) => setRows(list.allocations ?? []))
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const run = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    setError(null);
    try {
      await fn();
      load();
      return true;
    } catch (e) {
      setError((e as Error).message);
      return false;
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-8 px-8 pb-10 pt-6">
      <AdminHeader
        eyebrow="Administer"
        title="Allocations"
        subtitle="The registry of investable products — a fund exists only once it is registered here"
        action={
          <Button type="button" className={cn(TEAL_CTA)} onClick={() => setAdding((v) => !v)}>
            <Plus className="size-4" />
            Register allocation
          </Button>
        }
      />

      {error && (
        <p className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4" /> {error}
        </p>
      )}

      {adding && <RegisterForm busy={busy === "register"} onCancel={() => setAdding(false)} onSubmit={async (body) => (await run("register", () => registerAllocation(body))) && setAdding(false)} />}

      <section className="space-y-3">
        <p className="flex items-center gap-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
          Registry
          {rows && <span className="rounded-full bg-main-accent-t1/15 px-2 py-0.5 text-xs font-semibold text-main-accent-t1">{rows.length}</span>}
        </p>
        <Card>
          <CardContent className="p-0">
            <Settled
              loading={!rows}
              skeleton={
                <div className="p-6">
                  <Skeleton className="h-32 w-full" />
                </div>
              }
            >
              {!rows ? null : rows.length === 0 ? (
                <p className="p-8 text-center text-sm text-muted-foreground">
                  No allocations registered yet — until one is, every subscription is refused.
                </p>
              ) : (
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                      <th className="px-5 py-3 font-medium">Product</th>
                      <th className="px-5 py-3 font-medium">Service id</th>
                      <th className="px-5 py-3 font-medium">State</th>
                      <th className="px-5 py-3 font-medium">Unit cap</th>
                      <th className="px-5 py-3 text-right font-medium">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border">
                    {rows.map((row) => (
                      <AllocationRow
                        key={row.service}
                        row={row}
                        busy={busy === row.service}
                        editing={editing === row.service}
                        onEdit={() => setEditing((current) => (current === row.service ? null : row.service))}
                        onSave={async (body) => (await run(row.service, () => updateAllocation(body))) && setEditing(null)}
                        onToggle={() => run(row.service, () => setAllocationState(row.service, row.state === "open" ? "closed" : "open"))}
                      />
                    ))}
                  </tbody>
                </table>
              )}
            </Settled>
          </CardContent>
        </Card>
        <p className="max-w-3xl text-xs text-muted-foreground">
          Registration lands in <span className="font-mono-tech">draft</span>, which takes no money — opening is a separate decision. Closing stops new subscriptions only: investors can
          always redeem out of a closed product, so winding one down never strands their units. The service id is permanent once registered. The unit cap bounds how many units may
          ever be issued — resize it on the Valuation screen, where the issued figure it is judged against is on the same page.
        </p>
      </section>
    </div>
  );
}

function AllocationRow({
  row,
  busy,
  editing,
  onEdit,
  onSave,
  onToggle,
}: {
  row: Allocation;
  busy: boolean;
  editing: boolean;
  onEdit: () => void;
  onSave: (body: { service: string; title: string; summary: string }) => void;
  onToggle: () => void;
}) {
  const [title, setTitle] = useState(row.title);
  const [summary, setSummary] = useState(row.summary);

  return (
    <>
      <tr>
        <td className="px-5 py-3">
          <p className="font-medium">{row.title}</p>
          {row.summary && <p className="text-xs text-muted-foreground">{row.summary}</p>}
        </td>
        <td className="px-5 py-3 font-mono-tech text-xs text-muted-foreground">{row.service}</td>
        <td className="px-5 py-3">
          <span className={cn("inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium capitalize", STATE_TONE[row.state])} title={STATE_HINT[row.state]}>
            {row.state}
          </span>
        </td>
        {/* Read-only here: resizing the supply is a money decision and lives on the
            Valuation screen, next to the issued figure it has to be judged against. */}
        <td className="px-5 py-3 tabular-nums text-muted-foreground">{compactUnits(row.unit_cap)}</td>
        <td className="px-5 py-3">
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onEdit}>
              {editing ? "Cancel" : "Edit"}
            </Button>
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onToggle}>
              {busy ? <Loader2 className="size-4 animate-spin" /> : row.state === "open" ? "Close" : "Open"}
            </Button>
          </div>
        </td>
      </tr>
      {editing && (
        <tr className="bg-foreground/[0.03]">
          <td colSpan={5} className="px-5 py-4">
            <div className="flex flex-wrap items-end gap-3">
              {/* `flex flex-col`, not `block` + `space-y`: the uikit Input is `inline-flex`,
                  so a narrow one shares the line with its label unless the column is
                  explicit. */}
              <label className="flex w-56 flex-col gap-1.5">
                <span className="text-xs text-muted-foreground">Title</span>
                <Input value={title} onChange={(e) => setTitle(e.target.value)} className="w-full" />
              </label>
              <label className="flex min-w-56 flex-1 flex-col gap-1.5">
                <span className="text-xs text-muted-foreground">Summary</span>
                <Input value={summary} onChange={(e) => setSummary(e.target.value)} placeholder="One line for the catalog card" className="w-full" />
              </label>
              <Button type="button" className={cn(TEAL_CTA)} disabled={busy || !title.trim()} onClick={() => onSave({ service: row.service, title, summary })}>
                Save
              </Button>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

function RegisterForm({ busy, onCancel, onSubmit }: { busy: boolean; onCancel: () => void; onSubmit: (body: { service: string; title: string; summary: string }) => void }) {
  const [service, setService] = useState("");
  const [title, setTitle] = useState("");
  const [summary, setSummary] = useState("");

  // Mirrors `ServiceId::parse` — the hub rejects anything else, so say so before the
  // round-trip rather than surfacing a validation error after it.
  const slugOk = /^[A-Za-z0-9_-]{1,64}$/.test(service);

  return (
    <Card>
      <CardContent className="space-y-4 py-6">
        <div className="grid gap-4 md:grid-cols-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">Service id</span>
            <Input value={service} onChange={(e) => setService(e.target.value.trim())} placeholder="quy-nhon-fund" spellCheck={false} className="w-full font-mono-tech" />
            <span className={cn("text-xs", service && !slugOk ? "text-destructive" : "text-muted-foreground")}>
              {service && !slugOk ? "Letters, digits, - and _ only (1–64)" : "Permanent — every fund RPC keys on it"}
            </span>
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">Title</span>
            <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Quy Nhon Fund" className="w-full" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">Summary</span>
            <Input value={summary} onChange={(e) => setSummary(e.target.value)} placeholder="One line for the catalog card" className="w-full" />
          </label>
        </div>
        <div className="flex items-center gap-3">
          <p className="text-xs text-muted-foreground">Registers in draft — open it when the product is ready to take money.</p>
          <Button type="button" variant="outline" className="ml-auto" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="button" className={cn(TEAL_CTA)} disabled={busy || !slugOk || !title.trim()} onClick={() => onSubmit({ service, title, summary })}>
            {busy ? <Loader2 className="size-4 animate-spin" /> : null}
            Register
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
