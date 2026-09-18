"use client";

// Seat a person on a reserved allocation (#245). The reserved `fee` / `fund` allocations
// are the platform's own money, and who holds them is never one operator's decision: this
// form names the person and the units and OPENS A CONSILIUM. Nothing is minted here — when
// the quorum carries, the hub mints the grant as an in-kind issuance, and the cap table
// above follows when the relay posts it.

import { Loader2, TriangleAlert } from "lucide-react";
import { useId, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, Button, Field, FieldError, FieldLabel, Input } from "@evinvest/uikit";

import { openHolderGrant } from "@/entities/governance/model/governance-resource";
import type { Consilium } from "@/shared/contracts/governance";
import { errorMessage } from "@/shared/lib/api-client";
import { isPositiveWireDecimal } from "@/shared/lib/money";
import { ConsiliumOpened } from "@/views/admin/ui/consilium-opened";
import { UserPicker, type PickedUser } from "@/views/admin/ui/user-picker";

export function HolderGrantForm({ allocation }: { allocation: "fee" | "fund" }) {
  const t = useT();
  const id = useId();
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
        <Field>
          {/* No `htmlFor`: the picker's trigger sits behind a popover, so it is named by reference. */}
          <FieldLabel id={`${id}-holder`}>{t("admin.alloc.issue.field.holder")}</FieldLabel>
          <UserPicker value={holder} onPick={setHolder} labelledBy={`${id}-holder`} />
        </Field>
        <Field data-invalid={unitsProblem || undefined}>
          <FieldLabel htmlFor={`${id}-units`}>{t("admin.alloc.issue.field.units")}</FieldLabel>
          <Input id={`${id}-units`} inputMode="decimal" value={units} onChange={(e) => setUnits(e.target.value)} aria-invalid={unitsProblem || undefined} aria-describedby={unitsProblem ? `${id}-units-error` : undefined} className="tabular-nums" />
          {unitsProblem && <FieldError id={`${id}-units-error`}>{t("admin.alloc.issue.problem.units")}</FieldError>}
        </Field>
      </div>
      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
      {opened && <ConsiliumOpened consiliumId={opened.id} body={t("admin.revenue.grant.opened")} onDismiss={() => setOpened(null)} />}
      <Button type="button" className="w-full" disabled={busy || !sendable} onClick={() => void submit()}>
        {busy ? <Loader2 className="size-4 animate-spin" /> : null}
        {t("admin.revenue.grant.submit")}
      </Button>
      {!holder && <p className="text-center text-xs text-ink-soft">{t("admin.alloc.issue.reason.holder")}</p>}
    </div>
  );
}
