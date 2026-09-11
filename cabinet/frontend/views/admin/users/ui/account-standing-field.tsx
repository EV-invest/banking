"use client";

// The block/unblock control, and the three-way branch that is the whole point of it.
//
// The identity plane split one verb into two, because it had been doing two jobs with
// opposite requirements: an instant brake one operator must be able to pull, and a
// permanent judgement nobody should make alone. So this control cannot be a single
// Suspend/Reinstate toggle any more — what it may offer depends on WHY the account is
// blocked, which `suspended_by` records.
//
// It is read through `accountStanding`, which returns a union rather than booleans, so the
// third case cannot be quietly folded into one of the other two: an account suspended
// before the field existed carries no provenance, keeps the old never-lapsing semantics,
// and is neither a hold nor an owners' verdict. Branching on `status === "disabled"` alone
// — which is what this file used to do — offers that account the wrong button, and
// branching on two values of `suspended_by` files it under whichever was tested first.
//
// Its own file because the drawer it sits in was already well past the size where a reader
// can hold the whole thing in their head, and because this is the part with a rule in it.

import { Loader2, ShieldBan, ShieldCheck, ShieldQuestion } from "lucide-react";
import { useState } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Button } from "@evinvest/uikit";

import { holdUser, reinstateUser } from "@/entities/admin/api/admin-client";
import { openUserReinstatement, openUserSuspension } from "@/entities/governance/model/governance-resource";
import { formatMoment } from "@/shared/lib/datetime";
import { Link } from "@/shared/ui/cabinet-link";
import { TipAnchor } from "@/shared/tips";
import { type AccountStanding } from "@/views/admin/lib/format";
import { ReasonAction } from "@/views/admin/users/ui/reason-action";

/** The actions that need a sentence before they can be sent. `null` is "none pending". */
type Pending = "hold" | "suspension" | "reinstatement" | null;

export interface AccountStandingFieldProps {
  userId: string;
  standing: AccountStanding;
  /** The drawer's in-flight key, so two controls cannot be pressed at once. */
  busy: string | null;
  /** The drawer's action runner: sets busy, surfaces errors, revalidates the user tag. */
  run: (key: string, fn: () => Promise<unknown>) => Promise<void>;
}

export function AccountStandingField({ userId, standing, busy, run }: AccountStandingFieldProps) {
  const t = useT();
  const locale = useLocale();
  const [pending, setPending] = useState<Pending>(null);
  const [reason, setReason] = useState("");

  const working = busy === "standing";
  const submit = async (fn: () => Promise<unknown>) => {
    await run("standing", fn);
    setPending(null);
    setReason("");
  };

  return (
    <div className="space-y-2 border-t border-border pt-4">
      <p className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
        {t("admin.users.standing")}
        <TipAnchor anchor="admin.users.status.suspend" />
      </p>

      {/* What is true NOW, before any button. The sentence differs per case because the
          cases differ in what happens if nobody acts: a hold releases itself, a verdict
          does not, and a pre-split suspension does not either but can be lifted here. */}
      <p className="text-xs leading-relaxed text-muted-foreground">
        {standing.kind === "active" && t("admin.users.standingActive")}
        {standing.kind === "hold" && `${t("admin.users.standingHold")} ${t("admin.users.standingHoldLapses", { when: formatMoment(standing.expiresAt, locale) })}`}
        {standing.kind === "governance" && t("admin.users.standingGovernance")}
        {standing.kind === "legacy" && t("admin.users.standingLegacy")}
      </p>

      {/* The one-act lift. Offered for a hold and for a pre-split suspension — both of
          which `ReinstateUser` accepts — and never for the owners' verdict, which it
          refuses. Holding the button out there would be the `FAILED_PRECONDITION` the
          role control was already fixed for, on the surface where it matters most. */}
      {(standing.kind === "hold" || standing.kind === "legacy") && (
        <Button type="button" variant="outline" size="sm" className="w-full" disabled={working} onClick={() => void submit(() => reinstateUser(userId))}>
          {working && pending === null ? <Loader2 className="size-3.5 animate-spin" /> : <ShieldCheck className="size-3.5" />}
          {standing.kind === "hold" ? t("admin.users.liftHold") : t("admin.users.liftSuspension")}
        </Button>
      )}

      {/* The brake. Only on an account that is not already blocked — re-holding a held one
          would just restart a clock the owners are already voting against. */}
      {standing.kind === "active" && (
        <ReasonAction
          open={pending === "hold"}
          onOpen={() => setPending("hold")}
          onCancel={() => setPending(null)}
          label={t("admin.users.hold")}
          hint={t("admin.users.holdHint")}
          icon={<ShieldBan className="size-3.5" />}
          destructive
          busy={working}
          reason={reason}
          setReason={setReason}
          inputId="hold-reason"
          onSubmit={() => void submit(() => holdUser(userId, reason.trim()))}
        />
      )}

      {/* Making it stay, and undoing it once it has. Which of the two is offered follows
          the same branch: a hold (or an active account) can be proposed permanent; only a
          verdict needs a proposal to lift. A pre-split suspension gets neither — it is
          already permanent and already liftable in one act. */}
      {(standing.kind === "active" || standing.kind === "hold") && (
        <ReasonAction
          open={pending === "suspension"}
          onOpen={() => setPending("suspension")}
          onCancel={() => setPending(null)}
          label={t("admin.users.proposeSuspension")}
          hint={t("admin.users.proposeSuspensionHint")}
          icon={<ShieldBan className="size-3.5" />}
          busy={working}
          reason={reason}
          setReason={setReason}
          inputId="suspension-reason"
          onSubmit={() => void submit(() => openUserSuspension(userId, reason.trim()))}
        />
      )}

      {standing.kind === "governance" && (
        <ReasonAction
          open={pending === "reinstatement"}
          onOpen={() => setPending("reinstatement")}
          onCancel={() => setPending(null)}
          label={t("admin.users.proposeReinstatement")}
          hint={t("admin.users.proposeReinstatementHint")}
          icon={<ShieldQuestion className="size-3.5" />}
          busy={working}
          reason={reason}
          setReason={setReason}
          inputId="reinstatement-reason"
          onSubmit={() => void submit(() => openUserReinstatement(userId, reason.trim()))}
        />
      )}

      {standing.kind !== "active" && (
        <p className="text-xs leading-relaxed text-muted-foreground">
          {t("admin.users.standingOpened")}{" "}
          <Link href="/consilium" className="rounded-xs underline underline-offset-2 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
            {t("nav.consilium")}
          </Link>
          .
        </p>
      )}
    </div>
  );
}
