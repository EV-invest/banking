"use client";

// Propose a seed of the platform's capital (#245). A transfer to a rail's treasury address
// is nobody's until the owners say whose: this form names the transfer, the amount the
// chain must report and the person it is attributed to, and OPENS A CONSILIUM. Nothing is
// booked here — when the quorum carries, the hub books their deposit and their
// subscription into `fund` in one chain, so the seed is a person's units, never a
// balance without a holder.

import { Loader2, TriangleAlert } from "lucide-react";
import { useId, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, Button, Card, CardContent, Field, FieldDescription, FieldError, FieldLabel, Input } from "@evinvest/uikit";

import { proposeSeedCapital } from "@/entities/admin/api/admin-client";
import type { RailLiquidity, SeedCapitalProposal } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { isPositiveWireDecimal } from "@/shared/lib/money";
import { revalidateTag } from "@/shared/lib/resource";
import { StaggerItem } from "@/shared/ui/motion";
import { RailSelect, watchedRails } from "@/views/admin/treasury/ui/rail-select";
import { ConsiliumOpened } from "@/views/admin/ui/consilium-opened";
import { UserPicker, type PickedUser } from "@/views/admin/ui/user-picker";

export function SeedCapitalForm({ rails }: { rails: RailLiquidity[] | undefined }) {
  const t = useT();
  const id = useId();
  const [network, setNetwork] = useState("");
  const [txRef, setTxRef] = useState("");
  const [amount, setAmount] = useState("");
  const [depositor, setDepositor] = useState<PickedUser | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [opened, setOpened] = useState<SeedCapitalProposal | null>(null);

  // The amount is what the owners approve, so unlike the arrival form it is required —
  // and checked as the decimal the plane parses, not as a `Number`.
  const amountProblem = amount.trim() !== "" && !isPositiveWireDecimal(amount);
  const sendable = Boolean(network) && txRef.trim().length > 0 && isPositiveWireDecimal(amount);

  const submit = async () => {
    setBusy(true);
    setError(null);
    setOpened(null);
    try {
      const proposal = await proposeSeedCapital({
        tx_ref: txRef.trim(),
        network,
        expected_amount: amount.trim(),
        ...(depositor ? { depositor_user_id: depositor.userId } : {}),
      });
      setOpened(proposal);
      setTxRef("");
      setAmount("");
      // Only the room moves now; the treasury follows when the quorum carries.
      revalidateTag(TAG.consilium);
    } catch (e) {
      setError(errorMessage(e, t));
    } finally {
      setBusy(false);
    }
  };

  return (
    <StaggerItem as="section" className="space-y-3">
      <p className="text-xs font-semibold uppercase tracking-widest text-ink-soft">{t("admin.treasury.seed.title")}</p>
      <Card>
        <CardContent className="space-y-5 py-6">
          <p className="max-w-3xl text-sm text-ink-soft">{t("admin.treasury.seed.intro")}</p>
          <div className="grid gap-4 md:grid-cols-2">
            <RailSelect value={network} options={watchedRails(rails)} onChange={setNetwork} />
            <Field>
              <FieldLabel htmlFor={`${id}-ref`}>{t("admin.treasury.onchainRef")}</FieldLabel>
              {/* A format literal, not prose — it reads the same in every locale. */}
              <Input id={`${id}-ref`} value={txRef} onChange={(e) => setTxRef(e.target.value)} placeholder="0xhash:logIndex" spellCheck={false} className="font-mono-tech" />
            </Field>
            <Field data-invalid={amountProblem || undefined}>
              <FieldLabel htmlFor={`${id}-amount`}>{t("admin.treasury.seed.amount")}</FieldLabel>
              <Input id={`${id}-amount`} value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" aria-invalid={amountProblem || undefined} aria-describedby={amountProblem ? `${id}-amount-error` : undefined} className="tabular-nums" />
              {amountProblem && <FieldError id={`${id}-amount-error`}>{t("admin.treasury.seed.amountProblem")}</FieldError>}
            </Field>
            <Field>
              {/* No `htmlFor`: the picker's trigger sits behind a popover, so it is named by reference. */}
              <FieldLabel id={`${id}-depositor`}>{t("admin.treasury.seed.depositor")}</FieldLabel>
              <UserPicker value={depositor} onPick={setDepositor} labelledBy={`${id}-depositor`} />
              <FieldDescription>{t("admin.treasury.seed.depositorHint")}</FieldDescription>
            </Field>
          </div>

          {error && (
            <Alert variant="destructive">
              <TriangleAlert className="size-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
          {opened && <ConsiliumOpened consiliumId={opened.consilium_id} body={t("admin.treasury.seed.opened")} onDismiss={() => setOpened(null)} />}

          <Button type="button" className="ml-auto flex" disabled={busy || !sendable} onClick={() => void submit()}>
            {busy ? <Loader2 className="size-4 animate-spin" /> : null}
            {t("admin.treasury.seed.submit")}
          </Button>
        </CardContent>
      </Card>
    </StaggerItem>
  );
}
