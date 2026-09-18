"use client";

// The two ownership consilia's terms (#245), as the owners' approval page shows them. A
// holder grant: the units and the reserved allocation get the top of the card, the person
// is their money-plane id — this page is reached from an email with no session, so there
// is no directory to resolve a name from, and the id is what the digest binds. A seed: the
// amount on top, the chain reference in full (policy 13) and the depositor's id.

import { useLocale, useT } from "@evinvest/i18n/react";
import { Separator } from "@evinvest/uikit";

import type { HolderGrantTerms, SeedCapitalTerms } from "@/shared/contracts/governance";
import { hashPrefix } from "@/shared/lib/hash";
import { formatExactUsdt, formatUnits } from "@/shared/lib/money";
import { networkLabel } from "@/shared/lib/rail";
import { DetailRow, FieldCaption, FullAddress } from "@/views/approval/ui/approval-chrome";

/** Every hashed field must actually be on screen before the code field is (policy 12–13). */
export function renderableHolderGrant(terms: HolderGrantTerms | null | undefined): terms is HolderGrantTerms {
  return Boolean(terms?.allocation?.trim()) && Boolean(terms?.user_id?.trim()) && Boolean(terms?.units?.trim());
}

export function renderableSeedCapital(terms: SeedCapitalTerms | null | undefined): terms is SeedCapitalTerms {
  return Boolean(terms?.tx_ref?.trim()) && Boolean(terms?.network?.trim()) && Boolean(terms?.amount?.trim());
}

export function HolderGrantTermsBlock({ terms, payloadHash }: { terms: HolderGrantTerms; payloadHash: string }) {
  const t = useT();
  const locale = useLocale();
  const allocation = terms.allocation === "fee" || terms.allocation === "fund" ? t(`approval.holderGrant.allocation.${terms.allocation}`) : terms.allocation;
  return (
    <>
      <div className="flex flex-col gap-1.5">
        <FieldCaption>{t("approval.holderGrant.units")}</FieldCaption>
        <p className="text-4xl font-semibold leading-none tabular-nums text-ink">
          {/* The wire string, digit for digit — `payload_hash` covers the exact decimal. */}
          {formatUnits(terms.units, locale)}
          <span className="ml-2 text-base font-medium text-ink-soft">{allocation}</span>
        </p>
      </div>

      <div className="flex flex-col gap-2.5">
        <DetailRow label={t("approval.holderGrant.holder")} value={terms.user_id} mono />
        <DetailRow label={t("approval.payloadHash")} value={hashPrefix(payloadHash)} mono />
      </div>

      <p className="text-xs text-ink-soft">{t("approval.holderGrant.executes")}</p>
      <p className="text-xs text-ink-soft">{t("approval.holderGrant.payloadHashHint")}</p>

      <Separator />
    </>
  );
}

export function SeedCapitalTermsBlock({ terms, payloadHash }: { terms: SeedCapitalTerms; payloadHash: string }) {
  const t = useT();
  const locale = useLocale();
  return (
    <>
      <div className="flex flex-col gap-1.5">
        <FieldCaption>{t("approval.seedCapital.amount")}</FieldCaption>
        <p className="text-4xl font-semibold leading-none tabular-nums text-ink">
          {formatExactUsdt(terms.amount, locale)}
          <span className="ml-2 text-base font-medium text-ink-soft">USDT</span>
        </p>
      </div>

      <FullAddress label={t("approval.seedCapital.reference")} address={terms.tx_ref} />

      <div className="flex flex-col gap-2.5">
        <DetailRow label={t("approval.network")} value={networkLabel(terms.network)} />
        <DetailRow label={t("approval.seedCapital.depositor")} value={terms.depositor_user_id || "—"} mono />
        <DetailRow label={t("approval.payloadHash")} value={hashPrefix(payloadHash)} mono />
      </div>

      <p className="text-xs text-ink-soft">{t("approval.seedCapital.executes")}</p>
      <p className="text-xs text-ink-soft">{t("approval.seedCapital.payloadHashHint")}</p>

      <Separator />
    </>
  );
}
