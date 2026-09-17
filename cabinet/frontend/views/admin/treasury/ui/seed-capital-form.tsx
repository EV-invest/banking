"use client";

// Propose a seed of the platform's capital (#245). A transfer to a rail's treasury address
// is nobody's until the owners say whose: this form names the transfer, the amount the
// chain must report and the person it is attributed to, and OPENS A CONSILIUM. Nothing is
// booked here — when the quorum carries, the hub books their deposit and their
// subscription into `fund` in one chain, so the seed is a person's units, never a
// balance without a holder.

import { Loader2, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, Input } from "@evinvest/uikit";

import { proposeSeedCapital } from "@/entities/admin/api/admin-client";
import type { RailLiquidity, SeedCapitalProposal } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { cn } from "@/shared/lib/cn";
import { isPositiveWireDecimal } from "@/shared/lib/money";
import { revalidateTag } from "@/shared/lib/resource";
import { StaggerItem } from "@/shared/ui/motion";
import { RailSelect, watchedRails } from "@/views/admin/treasury/ui/rail-select";
import { ConsiliumOpened } from "@/views/admin/ui/consilium-opened";
import { UserPicker, type PickedUser } from "@/views/admin/ui/user-picker";

const TEAL_CTA = "bg-primary text-on-primary hover:bg-primary/90";

export function SeedCapitalForm({ rails }: { rails: RailLiquidity[] | undefined }) {
  const t = useT();
  const [network, setNetwork] = useState("");
  const [txRef, setTxRef] = useState("");
  const [amount, setAmount] = useState("");
  const [depositor, setDepositor] = useState<PickedUser | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [opened, setOpened] = useState<SeedCapitalProposal | null>(null);

  // The amount is what the owners approve, so unlike the arrival form it is required —
  // and checked as the decimal the plane parses, not as a `Number`.
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
            <label className="flex flex-col gap-1.5">
              <span className="text-sm text-ink-soft">{t("admin.treasury.onchainRef")}</span>
              {/* A format literal, not prose — it reads the same in every locale. */}
              <Input value={txRef} onChange={(e) => setTxRef(e.target.value)} placeholder="0xhash:logIndex" className="w-full" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-sm text-ink-soft">{t("admin.treasury.seed.amount")}</span>
              <Input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" className="w-full tabular-nums" />
              {amount.trim() !== "" && !isPositiveWireDecimal(amount) && <span className="text-xs text-accent-error">{t("admin.treasury.seed.amountProblem")}</span>}
            </label>
            <div className="flex flex-col gap-1.5">
              <span className="text-sm text-ink-soft">{t("admin.treasury.seed.depositor")}</span>
              <UserPicker value={depositor} onPick={setDepositor} />
              <span className="text-xs text-ink-soft">{t("admin.treasury.seed.depositorHint")}</span>
            </div>
          </div>

          {error && (
            <p className="flex items-center gap-2 text-sm text-accent-error">
              <TriangleAlert className="size-4 shrink-0" /> {error}
            </p>
          )}
          {opened && <ConsiliumOpened consiliumId={opened.consilium_id} body={t("admin.treasury.seed.opened")} onDismiss={() => setOpened(null)} />}

          <Button type="button" className={cn("ml-auto flex", TEAL_CTA)} disabled={busy || !sendable} onClick={() => void submit()}>
            {busy ? <Loader2 className="size-4 animate-spin" /> : null}
            {t("admin.treasury.seed.submit")}
          </Button>
        </CardContent>
      </Card>
    </StaggerItem>
  );
}
