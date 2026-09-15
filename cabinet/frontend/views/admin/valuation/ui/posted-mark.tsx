"use client";

// The receipt for a mark that went straight in (banking#253): the POST answers with the
// new mark and the form empties, so without this line the operator has nothing on screen
// saying the figure landed. Twin of the "proposed" receipt in `valuation-actions.tsx`, but
// this one leads with MARKED — the figure is already the fund's price.

import { CheckCircle2 } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button } from "@evinvest/uikit";

import type { FundNav } from "@/shared/contracts/admin";
import { formatExactUsdt, formatNav } from "@/shared/lib/money";

export function PostedMark({ mark, onClose }: { mark: FundNav; onClose: () => void }) {
  const t = useT();
  return (
    <Alert role="status" className="border-main-accent-t2/40 bg-main-accent-t2/10">
      <CheckCircle2 className="size-4 text-main-accent-t2" />
      <AlertTitle>{t("admin.valuation.postedTitle")}</AlertTitle>
      <AlertDescription className="gap-3 text-foreground">
        <p className="leading-relaxed tabular-nums">
          {t("admin.valuation.postedBody", { nav: formatNav(mark.nav), aum: formatExactUsdt(mark.aum) })}
        </p>
        <div className="flex flex-wrap gap-2">
          <Button type="button" size="sm" variant="ghost" onClick={onClose}>
            {t("ui.close")}
          </Button>
        </div>
      </AlertDescription>
    </Alert>
  );
}
