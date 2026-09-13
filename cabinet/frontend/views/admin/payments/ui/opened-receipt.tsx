"use client";

// What just happened, said accurately.
//
// The operator has clicked a button on a money surface, and the natural reading of any
// receipt is "it is done". This one leads with OPENED, says in plain words that nothing
// has moved, and names who has been asked — because that is the plane's answer, not the
// form's preview: an owner consilium (with the room linked, where the live tally is) or
// one investor's mailbox. It offers no "view payment" affordance beyond the list below,
// because until someone answers there is nothing more to see.

import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button } from "@evinvest/uikit";
import { CheckCircle2 } from "lucide-react";

import type { Payment } from "@/shared/contracts/payments";
import { expiresIn, formatMoment } from "@/shared/lib/datetime";
import { Link } from "@/shared/ui/cabinet-link";

export function OpenedReceipt({ payment, onDismiss }: { payment: Payment; onDismiss: () => void }) {
  const t = useT();
  const locale = useLocale();
  const consent = payment.consent;
  return (
    <Alert role="status" className="border-main-accent-t2/40 bg-main-accent-t2/10">
      <CheckCircle2 className="size-4 text-main-accent-t2" />
      <AlertTitle>{t("admin.payments.openedTitle")}</AlertTitle>
      <AlertDescription className="gap-3 text-foreground">
        <p className="leading-relaxed">
          {consent
            ? t("admin.payments.openedConsentBody", { email: consent.subject_email })
            : t("admin.payments.openedConsiliumBody")}
        </p>
        <p className="text-xs tabular-nums text-muted-foreground">
          {t("admin.payments.openedExpires", { at: formatMoment(payment.expires_at, locale), left: expiresIn(payment.expires_at, t) })}
        </p>
        <div className="flex flex-wrap gap-2">
          {payment.consilium_id && (
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
