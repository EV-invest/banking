"use client";

// Where the money on this screen is moved from, and where it is held.
//
// This screen used to carry the payout form. It is statistics now: paying the fund's
// earnings out is one case of a payment order, and the form that opens one lives on the
// Payments screen with every other case. The treasury is where the same claim appears
// among the fund's other balances. Two cards rather than two buttons, because each is a
// destination with a sentence's worth of reason, not an action.

import { ArrowLeftRight, ChevronRight, Landmark } from "lucide-react";
import type { ReactNode } from "react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent } from "@evinvest/uikit";

import { Link } from "@/shared/ui/cabinet-link";

export function WhereNext() {
  const t = useT();
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <LinkCard href="/admin/payments" icon={<ArrowLeftRight className="size-5" />} title={t("nav.payments")} body={t("admin.revenue.toPayments")} />
      <LinkCard href="/admin/treasury" icon={<Landmark className="size-5" />} title={t("nav.treasury")} body={t("admin.revenue.toTreasury")} />
    </div>
  );
}

function LinkCard({ href, icon, title, body }: { href: `/${string}`; icon: ReactNode; title: string; body: string }) {
  return (
    <Card className="relative transition-colors has-hover:bg-foreground/5 has-focus-visible:ring-2 has-focus-visible:ring-ring">
      <CardContent className="py-5">
        {/* The link is stretched over the whole card, so the hit target and the ring are the card. */}
        <Link href={href} className="flex items-start gap-3 outline-none after:absolute after:inset-0 after:rounded-xl">
          <span className="mt-0.5 shrink-0 text-main-accent-t1">{icon}</span>
          <span className="min-w-0 flex-1 space-y-1">
            <span className="block text-sm font-semibold text-foreground">{title}</span>
            <span className="block text-xs leading-relaxed text-muted-foreground">{body}</span>
          </span>
          <ChevronRight className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        </Link>
      </CardContent>
    </Card>
  );
}
