"use client";

import { useT } from "@evinvest/i18n/react";

import { Button, Card, CardContent, CardHeader, CardTitle } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { Link } from "@/shared/ui/cabinet-link";
import { StaggerItem } from "@/shared/ui/motion";
import { CARD_PAD } from "@/views/dashboard/lib/chrome";

// Two actions and nothing else: the "auto-deploy idle cash" switch that used to sit
// below them had no feature behind it (#397) and is gone until one exists. The one solid
// CTA on the page is the Deposit here — the topbar's shortcuts to the same two routes stay
// outline for that reason.
export function MoveMoneyCard({ className }: { className?: string }) {
  const t = useT();
  return (
    <StaggerItem as={Card} className={cn("gap-3.5 py-4 lg:gap-4 lg:py-5", className)}>
      <CardHeader className={CARD_PAD}>
        <CardTitle>{t("dash.moveMoney")}</CardTitle>
      </CardHeader>
      <CardContent className={cn("flex gap-2.5", CARD_PAD)}>
        <Button asChild className="flex-1">
          <Link href="/wallet/deposit">{t("ui.deposit")}</Link>
        </Button>
        <Button asChild variant="outline" className="flex-1">
          <Link href="/wallet/withdraw">{t("ui.withdraw")}</Link>
        </Button>
      </CardContent>
    </StaggerItem>
  );
}
