"use client";

// The one button at the foot of a catalog card. One uikit `Button` whatever the state —
// the list used to mix a filled button with a bare text link, which read as two
// different kinds of thing — and one destination per state, decided in
// `views/invest/lib/catalog-card`.

import { useT } from "@evinvest/i18n/react";
import { ArrowRight, ChartCandlestick, Lock, ShieldCheck } from "lucide-react";

import { Button } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { Link } from "@/shared/ui/cabinet-link";
import { supportHref } from "@/shared/ui/support-link";
import type { CardCta } from "@/views/invest/lib/catalog-card";
import { TEAL_CTA } from "@/views/invest/ui/atoms";

export function ProductCta({ cta, service, title }: { cta: CardCta; service: string; title: string }) {
  const t = useT();
  const page = `/invest/${encodeURIComponent(service)}` as const;
  const className = cn("mt-auto w-full", cta === "invest" && TEAL_CTA);

  switch (cta) {
    case "invest":
    case "manage":
    case "view":
      return (
        <Button asChild className={className} variant={cta === "invest" ? "primary" : "outline"}>
          <Link href={page}>
            {t(cta === "invest" ? "nav.invest" : cta === "manage" ? "ui.manage" : "ui.details")}
            <ArrowRight className="size-4" />
          </Link>
        </Button>
      );
    case "trade":
      return (
        <Button asChild className={className} variant="outline">
          <Link href={`${page}/trade`}>
            <ChartCandlestick className="size-4" />
            {t("invest.cta.trade")}
          </Link>
        </Button>
      );
    case "verify":
      return <VerifyToInvestCta className={className} />;
    case "locked":
      // There is no self-serve request for access: support raises the grant by hand, so
      // "ask" means the support mailbox, with the product named for them.
      return (
        <Button asChild className={className} variant="outline">
          <a href={supportHref({ subject: t("invest.cta.lockedSubject", { title }) })}>
            <Lock className="size-4" />
            {t("invest.cta.locked")}
          </a>
        </Button>
      );
  }
}

/**
 * The muted way to unlock investing for a tier-0 caller — the card's `verify` state and the
 * product page's Subscribe slot (#395) say it in the same words and lead to the same place.
 * The profile's identity card is where a start is offered — the same dialog the wallet opens,
 * reached through its own surface rather than a fourth copy of it.
 */
export function VerifyToInvestCta({ className }: { className?: string }) {
  const t = useT();
  return (
    <Button asChild className={className} variant="outline">
      <Link href="/profile">
        <ShieldCheck className="size-4" />
        {t("invest.cta.verify")}
      </Link>
    </Button>
  );
}
