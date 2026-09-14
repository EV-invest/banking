"use client";

// The change on its way for one product, and the one thing an operator can do to it.
//
// Cancelling is not the reversible click a single ghost button makes it look like: a
// change still awaiting the owners takes its consilium down with it, voiding every
// approval collected so far. So the button opens a second step in place — no
// `window.confirm`, whose text no catalogue can translate.

import { Loader2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, Badge, Button, Card, CardContent } from "@evinvest/uikit";

import { cancelFeePolicyChange } from "@/entities/admin/api/admin-client";
import type { FeePolicyChange } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { cn } from "@/shared/lib/cn";
import { formatMoment } from "@/shared/lib/datetime";
import { revalidateTag } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";
import { changeStateLabel, changeStateTone, termsSummary } from "@/views/admin/fees/lib/format";
import { ago } from "@/views/admin/lib/format";

export function PendingCard({ change }: { change: FeePolicyChange }) {
  const t = useT();
  const locale = useLocale();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const awaiting = change.state === "awaiting_consilium";
  // The button that opened the confirmation is gone the moment it opens, and focus would
  // fall to `body`. It lands on the SAFE answer instead: Enter from there backs out, and a
  // keyboard user has to aim at the destructive one deliberately.
  const keepRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (confirming) keepRef.current?.focus();
  }, [confirming]);

  async function cancel() {
    setBusy(true);
    setProblem(null);
    try {
      await cancelFeePolicyChange({ service: change.service, change_id: change.id });
      // The policy loses its `pending`, the history row turns `cancelled`, and the form
      // beneath unblocks — all readers of the tag.
      revalidateTag(TAG.adminFees, TAG.consilium);
      setConfirming(false);
    } catch (e) {
      setProblem(errorMessage(e, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="h-fit border-main-accent-t1/40">
      <CardContent className="space-y-3 py-6">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <p className="text-sm font-semibold">{t("admin.fees.pendingTitle")}</p>
          <Badge variant="outline" className={cn("shrink-0", changeStateTone(change.state))}>
            {changeStateLabel(change.state, t)}
          </Badge>
        </div>
        <p className="text-sm tabular-nums">{termsSummary(change, t)}</p>
        <p className="text-xs text-muted-foreground">
          {awaiting
            ? t("admin.fees.pending.awaiting", { version: change.version })
            : t("admin.fees.pending.scheduled", { version: change.version, at: formatMoment(change.effective_from, locale) })}
        </p>
        {change.reason.trim() && (
          <blockquote className="whitespace-pre-line border-l-2 border-main-accent-t3/60 pl-3 text-sm leading-relaxed">{change.reason.trim()}</blockquote>
        )}
        <p className="text-xs text-muted-foreground">{t("admin.fees.pending.requested", { when: ago(change.requested_at, t) })}</p>

        {problem && <p className="text-xs text-destructive">{problem}</p>}

        {confirming ? (
          <Alert variant="destructive" role="status">
            <AlertDescription className="gap-3">
              <p className="text-sm leading-relaxed">{t(awaiting ? "admin.fees.cancelWarningAwaiting" : "admin.fees.cancelWarning")}</p>
              <div className="flex flex-col gap-2.5 sm:flex-row">
                <Button variant="destructive" size="sm" disabled={busy} onClick={() => void cancel()}>
                  {busy && <Loader2 className="size-4 animate-spin" />}
                  {t("admin.fees.cancelConfirm")}
                </Button>
                <Button ref={keepRef} variant="ghost" size="sm" disabled={busy} onClick={() => setConfirming(false)}>
                  {t("ui.cancel")}
                </Button>
              </div>
            </AlertDescription>
          </Alert>
        ) : (
          <div className="flex flex-wrap gap-2">
            {awaiting && (
              <Button asChild size="sm" variant="outline">
                <Link href="/consilium">{t("admin.payments.openConsilium")}</Link>
              </Button>
            )}
            <Button variant="ghost" size="sm" onClick={() => setConfirming(true)}>
              {t("admin.fees.cancelChange")}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
