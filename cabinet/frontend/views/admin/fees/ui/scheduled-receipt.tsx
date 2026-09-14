"use client";

// What just happened, said accurately.
//
// The operator has clicked a button on a pricing surface, and the natural reading of any
// receipt is "it is done". This one says in plain words that the fund still charges its
// current terms, and names what has to happen first — a moment that has to arrive, or
// the owners who have to agree (with the room linked, where the live tally is). The
// sibling of `views/admin/payments/ui/opened-receipt.tsx`, for the same reason.

import { CalendarClock, Users } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button } from "@evinvest/uikit";

import type { FeePolicyChange } from "@/shared/contracts/admin";
import { formatMoment } from "@/shared/lib/datetime";
import { Link } from "@/shared/ui/cabinet-link";

export function ScheduledReceipt({ change, onDismiss }: { change: FeePolicyChange; onDismiss: () => void }) {
  const t = useT();
  const locale = useLocale();
  const awaiting = change.state === "awaiting_consilium";
  return (
    <Alert role="status" className="border-main-accent-t2/40 bg-main-accent-t2/10">
      {awaiting ? <Users className="size-4 text-main-accent-t2" /> : <CalendarClock className="size-4 text-main-accent-t2" />}
      <AlertTitle>{t(awaiting ? "admin.fees.awaitingTitle" : "admin.fees.scheduledTitle")}</AlertTitle>
      <AlertDescription className="gap-3 text-foreground">
        <p className="leading-relaxed">
          {awaiting
            ? t("admin.fees.awaitingBody", { version: change.version })
            : t("admin.fees.scheduledBody", { version: change.version, at: formatMoment(change.effective_from, locale) })}
        </p>
        <div className="flex flex-wrap gap-2">
          {awaiting && (
            <Button asChild size="sm" variant="outline">
              <Link href="/consilium">{t("admin.payments.openConsilium")}</Link>
            </Button>
          )}
          <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
            {t("ui.close")}
          </Button>
        </div>
      </AlertDescription>
    </Alert>
  );
}
