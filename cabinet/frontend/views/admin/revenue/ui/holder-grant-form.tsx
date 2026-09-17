"use client";

// Seat a person on a reserved allocation (#245). The reserved `fee` / `fund` allocations
// are the platform's own money, and who holds them is never one operator's decision: this
// form names the person and the units and OPENS A CONSILIUM. Nothing is minted here — when
// the quorum carries, the hub mints the grant as an in-kind issuance, and the cap table
// above follows when the relay posts it.

import { Loader2, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Input } from "@evinvest/uikit";

import { openHolderGrant } from "@/entities/governance/model/governance-resource";
import type { Consilium } from "@/shared/contracts/governance";
import { errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { isPositiveWireDecimal } from "@/shared/lib/money";
import { ConsiliumOpened } from "@/views/admin/ui/consilium-opened";
import { UserPicker, type PickedUser } from "@/views/admin/ui/user-picker";

const TEAL_CTA = "bg-primary text-on-primary hover:bg-primary/90";

export function HolderGrantForm({ allocation }: { allocation: "fee" | "fund" }) {
  const t = useT();
  const [holder, setHolder] = useState<PickedUser | null>(null);
  const [units, setUnits] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [opened, setOpened] = useState<Consilium | null>(null);

  const unitsProblem = units.trim() !== "" && !isPositiveWireDecimal(units);
  const sendable = holder !== null && isPositiveWireDecimal(units);

  const submit = async () => {
    if (!holder) return;
    setBusy(true);
    setError(null);
    setOpened(null);
    try {
      setOpened(await openHolderGrant({ allocation, user_id: holder.userId, units: units.trim() }));
      // The person stays for a second grant in a series; the figure clears.
      setUnits("");
    } catch (e) {
      setError(errorMessage(e, t));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3 rounded-lg border border-border bg-secondary p-3">
      <p className="text-xs text-ink-soft">{t("admin.revenue.grant.intro")}</p>
      <div className="grid gap-2.5">
        <div className="flex flex-col gap-1.5">
          <span className="text-xs text-ink-soft">{t("admin.alloc.issue.field.holder")}</span>
          <UserPicker value={holder} onPick={setHolder} />
        </div>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-ink-soft">{t("admin.alloc.issue.field.units")}</span>
          <Input inputMode="decimal" value={units} onChange={(e) => setUnits(e.target.value)} className="w-full tabular-nums" />
          {unitsProblem && <span className="text-xs text-accent-error">{t("admin.alloc.issue.problem.units")}</span>}
        </label>
      </div>
      {error && (
        <p className="flex items-center gap-2 text-xs text-accent-error">
          <TriangleAlert className="size-3.5 shrink-0" /> {error}
        </p>
      )}
      {opened && <ConsiliumOpened consiliumId={opened.id} body={t("admin.revenue.grant.opened")} onDismiss={() => setOpened(null)} />}
      <Button type="button" className={cn("w-full", TEAL_CTA)} disabled={busy || !sendable} onClick={() => void submit()}>
        {busy ? <Loader2 className="size-4 animate-spin" /> : null}
        {t("admin.revenue.grant.submit")}
      </Button>
      {!holder && <p className="text-center text-xs text-ink-soft">{t("admin.alloc.issue.reason.holder")}</p>}
    </div>
  );
}
