"use client";

import { Loader2, Plus } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, Input, Skeleton } from "@evinvest/uikit";

import { registerAllocation, setAllocationState, updateAllocation } from "@/entities/admin/api/admin-client";
import { adminAllocationsResource } from "@/entities/admin/model/admin-resource";
import type { Allocation, AllocationState } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { cn } from "@/shared/lib/cn";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { Settled, StaggerItem } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { compactUnits, stateLabel } from "@/views/admin/lib/format";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

// The registry's own vocabulary, rendered. `closed` is amber rather than destructive:
// it stops new subscriptions but investors can still redeem out of it, so it is a
// wind-down, not a failure.
const STATE_TONE: Record<AllocationState, string> = {
  draft: "border-border text-muted-foreground",
  open: "border-main-accent-t2/40 bg-main-accent-t2/10 text-main-accent-t2",
  closed: "border-main-accent-t3/40 bg-main-accent-t3/10 text-main-accent-t3",
};

// Catalogue keys, not finished prose: this map is module scope, so it holds what to say
// rather than the words, and the row resolves it against the reader's locale.
const STATE_HINT: Record<AllocationState, string> = {
  draft: "admin.alloc.hint.draft",
  open: "admin.alloc.hint.open",
  closed: "admin.alloc.hint.closed",
};

export function AllocationsView() {
  const t = useT();
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);

  const read = useResource(adminAllocationsResource);
  const rows = read.data ? (read.data.allocations ?? []) : null;
  const error = actionError ?? (read.data || !read.error ? null : errorMessage(read.error, t));

  const run = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    setActionError(null);
    try {
      await fn();
      // Registering, renaming or opening a fund also changes what investors see: the rail's
      // Products group, the fund picker, and the names on the activity rows all read the
      // investor-facing catalog. Naming that tag is what keeps an operator change from
      // sitting invisible behind a five-minute window on every open investor tab.
      revalidateTag(TAG.catalog);
      await read.refresh();
      return true;
    } catch (e) {
      setActionError(errorMessage(e, t));
      return false;
    } finally {
      setBusy(null);
    }
  };

  return (
    <AdminScreen className="space-y-8">
      <AdminHeader
        eyebrow={t("admin.eyebrow.administer")}
        title={t("nav.allocations")}
        subtitle={t("admin.alloc.subtitle")}
        action={
          <Button type="button" className={cn(TEAL_CTA)} onClick={() => setAdding((v) => !v)}>
            <Plus className="size-4" />
            {t("admin.alloc.register")}
          </Button>
        }
      />

      {error && <ResourceError message={error} />}

      {adding && <RegisterForm busy={busy === "register"} onCancel={() => setAdding(false)} onSubmit={async (body) => (await run("register", () => registerAllocation(body))) && setAdding(false)} />}

      <StaggerItem as="section" className="space-y-3">
        <p className="flex items-center gap-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
          {t("admin.alloc.registry")}
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
                <p className="p-8 text-center text-sm text-muted-foreground">{t("admin.alloc.empty")}</p>
              ) : (
                <table className="w-full text-sm">
                  <thead>
                    {/* i18n-max: 14 per header — auto-layout table with no scroll wrapper;
                        a long header widens its column and squeezes the Product cell. */}
                    <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                      <th className="px-5 py-3 font-medium">{t("admin.alloc.col.product")}</th>
                      <th className="px-5 py-3 font-medium">{t("admin.alloc.col.serviceId")}</th>
                      <th className="px-5 py-3 font-medium">{t("admin.col.state")}</th>
                      <th className="px-5 py-3 font-medium">{t("admin.alloc.col.unitCap")}</th>
                      <th className="px-5 py-3 text-right font-medium">{t("admin.col.actions")}</th>
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
        {/* One key for the whole paragraph, with the state name interpolated: a translator
            has to be able to move `draft` to wherever the sentence puts it in their
            language, which splitting the note around the `<span>` would forbid. The cost
            is the word's mono styling, which is decoration rather than meaning here. */}
        <p className="max-w-3xl text-xs text-muted-foreground">{t("admin.alloc.footnote", { state: t("admin.state.draft") })}</p>
      </StaggerItem>
    </AdminScreen>
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
  const t = useT();
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
          {/* i18n-max: 12 — a chip in a table cell. The value is authored in display
              case rather than lowercased and fixed with CSS `capitalize`: that rule
              title-cases every word, which is invisible on a one-word English enum and
              wrong the moment it is translated ("en cours" → "En Cours"). */}
          <span className={cn("inline-flex items-center whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium", STATE_TONE[row.state])} title={t(STATE_HINT[row.state])}>
            {stateLabel(row.state, t)}
          </span>
        </td>
        {/* Read-only here: resizing the supply is a money decision and lives on the
            Valuation screen, next to the issued figure it has to be judged against. */}
        <td className="px-5 py-3 tabular-nums text-muted-foreground">{compactUnits(row.unit_cap)}</td>
        <td className="px-5 py-3">
          {/* i18n-max: 12 per verb — two `shrink-0` Buttons share this cell, and every
              character widens the Actions column at the Product column's expense. */}
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onEdit}>
              {editing ? t("ui.cancel") : t("ui.edit")}
            </Button>
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onToggle}>
              {busy ? <Loader2 className="size-4 animate-spin" /> : row.state === "open" ? t("admin.alloc.close") : t("admin.alloc.open")}
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
                <span className="text-xs text-muted-foreground">{t("admin.alloc.field.title")}</span>
                <Input value={title} onChange={(e) => setTitle(e.target.value)} className="w-full" />
              </label>
              <label className="flex min-w-56 flex-1 flex-col gap-1.5">
                <span className="text-xs text-muted-foreground">{t("admin.alloc.field.summary")}</span>
                <Input value={summary} onChange={(e) => setSummary(e.target.value)} placeholder={t("admin.alloc.placeholder.summary")} className="w-full" />
              </label>
              <Button type="button" className={cn(TEAL_CTA)} disabled={busy || !title.trim()} onClick={() => onSave({ service: row.service, title, summary })}>
                {t("ui.save")}
              </Button>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

function RegisterForm({ busy, onCancel, onSubmit }: { busy: boolean; onCancel: () => void; onSubmit: (body: { service: string; title: string; summary: string }) => void }) {
  const t = useT();
  const [service, setService] = useState("");
  const [title, setTitle] = useState("");
  const [summary, setSummary] = useState("");

  // Mirrors `ServiceId::parse` — the hub rejects anything else, so say so before the
  // round-trip rather than surfacing a validation error after it.
  const slugOk = /^[A-Za-z0-9_-]{1,64}$/.test(service);

  return (
    // A `StaggerItem` although it is not part of the page's own arrival: it mounts when
    // the operator asks for it, and the parent's variants carry it in the same way.
    <StaggerItem as={Card}>
      <CardContent className="space-y-4 py-6">
        <div className="grid gap-4 md:grid-cols-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">{t("admin.alloc.col.serviceId")}</span>
            {/* The two placeholders are format examples, not prose — a slug and a proper
                noun — so they stay as they are in every locale. */}
            <Input value={service} onChange={(e) => setService(e.target.value.trim())} placeholder="quy-nhon-fund" spellCheck={false} className="w-full font-mono-tech" />
            <span className={cn("text-xs", service && !slugOk ? "text-destructive" : "text-muted-foreground")}>
              {service && !slugOk ? t("admin.alloc.slugInvalid") : t("admin.alloc.slugHint")}
            </span>
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">{t("admin.alloc.field.title")}</span>
            <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Quy Nhon Fund" className="w-full" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">{t("admin.alloc.field.summary")}</span>
            <Input value={summary} onChange={(e) => setSummary(e.target.value)} placeholder={t("admin.alloc.placeholder.summary")} className="w-full" />
          </label>
        </div>
        <div className="flex items-center gap-3">
          <p className="min-w-0 text-xs text-muted-foreground">{t("admin.alloc.registerHint")}</p>
          {/* i18n-max: 12 per verb — both Buttons are `shrink-0` beside the hint above. */}
          <Button type="button" variant="outline" className="ml-auto" onClick={onCancel}>
            {t("ui.cancel")}
          </Button>
          <Button type="button" className={cn(TEAL_CTA)} disabled={busy || !slugOk || !title.trim()} onClick={() => onSubmit({ service, title, summary })}>
            {busy ? <Loader2 className="size-4 animate-spin" /> : null}
            {t("admin.alloc.registerSubmit")}
          </Button>
        </div>
      </CardContent>
    </StaggerItem>
  );
}
