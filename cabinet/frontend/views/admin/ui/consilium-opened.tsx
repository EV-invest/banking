"use client";

// What just happened when a console form opened an owners' consilium, said accurately.
//
// The natural reading of any receipt on a money surface is "it is done". This one leads
// with OPENED, says in plain words that nothing has moved, names the consilium the owners
// were emailed about, and links the room where the live tally is. The Payments and Fees
// receipts say the same thing in their own words; this is the one the ownership forms
// share (#245), where the subject is a person seated or capital seeded.

import { CheckCircle2 } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button } from "@evinvest/uikit";

import { Link } from "@/shared/ui/cabinet-link";

export function ConsiliumOpened({ consiliumId, body, onDismiss }: { consiliumId: string; body: string; onDismiss: () => void }) {
  const t = useT();
  return (
    <Alert role="status" variant="success">
      <CheckCircle2 className="size-4" />
      <AlertTitle>{t("admin.consilium.openedTitle", "Opened — nothing has moved")}</AlertTitle>
      <AlertDescription className="gap-3">
        <p className="leading-relaxed">{body}</p>
        {/* The id is what the audit trail and the emails carry, so it is shown verbatim. */}
        <p className="font-mono-tech text-xs text-ink-soft" title={consiliumId}>
          {t("admin.consilium.openedId", "Consilium {id}", { id: consiliumId })}
        </p>
        <div className="flex flex-wrap gap-2 text-ink">
          <Button asChild size="sm" variant="outline">
            <Link href="/consilium">{t("admin.consilium.open", "Open the consilium")}</Link>
          </Button>
          <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
            {t("ui.close", "Close")}
          </Button>
        </div>
      </AlertDescription>
    </Alert>
  );
}
