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

import type { Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button } from "@evinvest/uikit";

import type { FeePolicyChange } from "@/shared/contracts/admin";
import { formatMoment } from "@/shared/lib/datetime";
import { Link } from "@/shared/ui/cabinet-link";
import { receiptKind } from "@/views/admin/fees/lib/receipt";

type ReceiptKind = "awaiting" | "scheduled" | "immediate";
type ReceiptArgs = { version: number; at: string };

const titles = (t: Translate): Record<ReceiptKind, string> => ({
  awaiting: t("admin.fees.awaitingTitle", "Sent to the owners — nothing has changed yet"),
  scheduled: t("admin.fees.scheduledTitle", "Scheduled — nothing has changed yet"),
  immediate: t("admin.fees.immediateTitle", "In force within a minute — nobody to notify"),
});
const bodies = (t: Translate, args: ReceiptArgs): Record<ReceiptKind, string> => ({
  awaiting: t("admin.fees.awaitingBody", "The owners have each been emailed a link. Version {version} is scheduled only once more than half of them approve, and binds 24 hours after that while anyone holds units. The live tally is in the consilium.", args),
  scheduled: t("admin.fees.scheduledBody", "Version {version} takes effect on {at}. Every holder is being emailed the notice; until then the fund charges its current terms.", args),
  immediate: t("admin.fees.immediateBody", "Nobody holds units in this product, so no notice was due: version {version} takes effect now and is applied within a minute. Until it is, it can still be cancelled below. The first investor to subscribe reads these terms.", args),
});
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
    <Alert role="status" variant="success">
      <Icon className="size-4" />
      <AlertTitle>{titles(t)[kind]}</AlertTitle>
      <AlertDescription className="gap-3">
        <p className="leading-relaxed">{bodies(t, { version: change.version, at: formatMoment(change.effective_from, locale) })[kind]}</p>
        <div className="flex flex-wrap gap-2 text-ink">
          {awaiting && (
            <Button asChild size="sm" variant="outline">
              <Link href="/consilium">{t("admin.payments.openConsilium", "Open the consilium")}</Link>
            </Button>
          )}
          <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
            {t("ui.close", "Close")}
          </Button>
        </div>
      </AlertDescription>
    </Alert>
  );
}
