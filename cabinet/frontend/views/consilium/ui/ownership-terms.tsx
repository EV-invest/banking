"use client";

// The two ownership consilia's terms (#245), as the owners' room shows them beside the
// tally. A holder grant seats a person on a reserved allocation: the allocation, the
// person and the units are the whole of the hashed subject. A seed attributes a chain
// transfer to a person: the reference is shown in FULL, the same rule as a payout's
// address — an owner who checks it here and approves it from their mailbox must be
// looking at the same characters (policy 13).

import type { Locale, Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";

import type { HolderGrantTerms, SeedCapitalTerms } from "@/shared/contracts/governance";
import { formatExactUsdt, formatUnits } from "@/shared/lib/money";
import { networkLabel } from "@/shared/lib/rail";

/** The reserved allocations by name; one the hub reserves later falls back to its slug. */
export function reservedAllocationLabel(allocation: string, t: Translate): string {
  if (allocation === "fee" || allocation === "fund") return t(`consilium.holderGrant.allocation.${allocation}`);
  return allocation;
}

export function HolderGrantTermsBlock({ terms }: { terms: HolderGrantTerms }) {
  const t = useT();
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1">
        <span className="text-xs font-medium text-ink-soft">{t("consilium.holderGrant.allocation")}</span>
        <span className="text-sm font-medium text-ink">{reservedAllocationLabel(terms.allocation, t)}</span>
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-xs font-medium text-ink-soft">{t("consilium.holderGrant.holder")}</span>
        {/* The money-plane id the terms stored — the digest binds it, so it is shown as-is. */}
        <span className="break-all font-mono-tech text-xs text-ink">{terms.user_id}</span>
      </div>
      <p className="text-xs text-ink-soft">{t("consilium.holderGrant.executes")}</p>
    </div>
  );
}

export function SeedCapitalTermsBlock({ terms }: { terms: SeedCapitalTerms }) {
  const t = useT();
  return (
    <div className="flex flex-col gap-3">
      <p className="break-all rounded-lg border border-border bg-secondary px-3 py-2.5 font-mono-tech text-xs leading-relaxed text-ink">{terms.tx_ref || "—"}</p>
      <div className="flex flex-col gap-1.5 text-xs text-ink-soft">
        <span className="tabular-nums">{t("consilium.payout.network", { network: networkLabel(terms.network) })}</span>
        <span className="break-all font-mono-tech">{t("consilium.seedCapital.depositor", { id: terms.depositor_user_id })}</span>
      </div>
      <p className="text-xs text-ink-soft">{t("consilium.seedCapital.executes")}</p>
    </div>
  );
}

/** The one-line name of a settled grant in the room's history: "1,000.00 units · Platform fees". */
export function holderGrantWords(terms: HolderGrantTerms, t: Translate, locale: Locale): string {
  return `${formatUnits(terms.units, locale)} · ${reservedAllocationLabel(terms.allocation, t)}`;
}

/** The one-line name of a settled seed in the room's history: "10,000.00 USDT · BEP20". */
export function seedCapitalWords(terms: SeedCapitalTerms, locale: Locale): string {
  return `${formatExactUsdt(terms.amount, locale)} USDT · ${networkLabel(terms.network)}`;
}

/** The headline figure of an open ownership consilium: units for a grant, USDT for a seed. */
export function OwnershipHeadline({ grant, seed }: { grant: HolderGrantTerms | null; seed: SeedCapitalTerms | null }) {
  const t = useT();
  const locale = useLocale();
  return (
    <p className="text-2xl font-semibold leading-none tabular-nums text-ink">
      {grant ? formatUnits(grant.units, locale) : formatExactUsdt(seed?.amount, locale)}
      <span className="ml-2 text-sm font-medium text-ink-soft">{grant ? t("consilium.holderGrant.unitsWord") : "USDT"}</span>
    </p>
  );
}
