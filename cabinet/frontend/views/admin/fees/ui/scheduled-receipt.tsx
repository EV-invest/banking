"use client";

// What just happened, said accurately.
//
// The operator has clicked a button on a pricing surface, and the natural reading of any
// receipt is "it is done". This one says in plain words that the fund still charges its
// current terms, and names what has to happen first — a moment that has to arrive, or
// the owners who have to agree (with the room linked, where the live tally is). The one
// exception is a product nobody holds: no notice was due, the plane set the moment to the
// request itself, and "every holder is being emailed" would be a lie about people who do
// not exist — so that answer is told apart (`lib/receipt.ts`) and said as what it is. The
// sibling of `views/admin/payments/ui/opened-receipt.tsx`, for the same reason.

import { CalendarClock, Users, Zap } from "lucide-react";
import { useState } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button } from "@evinvest/uikit";

import type { FeePolicyChange } from "@/shared/contracts/admin";
import { formatMoment } from "@/shared/lib/datetime";
import { Link } from "@/shared/ui/cabinet-link";
import { receiptKind } from "@/views/admin/fees/lib/receipt";

const TITLE_KEY = { awaiting: "admin.fees.awaitingTitle", scheduled: "admin.fees.scheduledTitle", immediate: "admin.fees.immediateTitle" } as const;
const BODY_KEY = { awaiting: "admin.fees.awaitingBody", scheduled: "admin.fees.scheduledBody", immediate: "admin.fees.immediateBody" } as const;
const ICON = { awaiting: Users, scheduled: CalendarClock, immediate: Zap } as const;

export function ScheduledReceipt({ change, onDismiss }: { change: FeePolicyChange; onDismiss: () => void }) {
  const t = useT();
  const locale = useLocale();
  // Read once: the receipt answers the click, and a moment that binds seconds after it
  // must not turn from "now" into "scheduled for 14:03" on the next render.
  const [kind] = useState(() => receiptKind(change, Math.floor(Date.now() / 1000)));
  const awaiting = kind === "awaiting";
  const Icon = ICON[kind];
  return (
    <Alert role="status" className="border-main-accent-t2/40 bg-main-accent-t2/10">
      <Icon className="size-4 text-main-accent-t2" />
      <AlertTitle>{t(TITLE_KEY[kind])}</AlertTitle>
      <AlertDescription className="gap-3 text-foreground">
        <p className="leading-relaxed">{t(BODY_KEY[kind], { version: change.version, at: formatMoment(change.effective_from, locale) })}</p>
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
