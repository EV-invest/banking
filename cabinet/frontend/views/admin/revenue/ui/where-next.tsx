"use client";

// Where the money on this screen is moved from, and where it is held.
//
// Paying the platform's earnings out is one case of a payment order (from `service:fee`),
// and the form that opens one lives on the Payments screen with every other case. The
// treasury is where the same allocation appears beside the products and `fund`. Two
// cards rather than two buttons, because each is a destination with a sentence's worth
// of reason, not an action.

import { ArrowLeftRight, ChevronRight, Landmark } from "lucide-react";
import type { ReactNode } from "react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent } from "@evinvest/uikit";

import { Link } from "@/shared/ui/cabinet-link";

export function WhereNext() {
  const t = useT();
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <LinkCard href="/admin/payments" icon={<ArrowLeftRight className="size-5" />} title={t("nav.payments", "Payments")} body={t("admin.revenue.toPayments", "Moving the platform's earnings — into a product, or to an investor — is a payment order the owners authorise. Open one there. Cash reaches the chain only through a holder's redemption and their own withdrawal.")} />
      <LinkCard href="/admin/treasury" icon={<Landmark className="size-5" />} title={t("nav.treasury", "Treasury")} body={t("admin.revenue.toTreasury", "The fee allocation beside the platform's other balances: custody per rail, user claims, every allocation and its holders.")} />
    </div>
  );
}

function LinkCard({ href, icon, title, body }: { href: `/${string}`; icon: ReactNode; title: string; body: string }) {
  return (
    <Card className="relative transition-colors has-hover:bg-ink/5 has-focus-visible:ring-2 has-focus-visible:ring-ring">
      <CardContent className="py-5">
        {/* The link is stretched over the whole card, so the hit target and the ring are the card. */}
        <Link href={href} className="flex items-start gap-3 outline-none after:absolute after:inset-0 after:rounded-xl">
          <span className="mt-0.5 shrink-0 text-primary-ink">{icon}</span>
          <span className="min-w-0 flex-1 space-y-1">
            <span className="block text-sm font-semibold text-ink">{title}</span>
            <span className="block text-xs leading-relaxed text-ink-soft">{body}</span>
          </span>
          <ChevronRight className="mt-0.5 size-4 shrink-0 text-ink-soft" />
        </Link>
      </CardContent>
    </Card>
  );
}
