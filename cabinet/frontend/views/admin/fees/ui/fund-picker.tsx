"use client";

// One chip per fund, with its rate inline so the operator can see at a glance which
// products are priced, which are still charging nothing, and which have a change on the way.

import { useT } from "@evinvest/i18n/react";

import type { FeePolicy } from "@/shared/contracts/admin";
import { pct } from "@/shared/lib/rate";
import { StaggerItem } from "@/shared/ui/motion";
import { isPendingChange } from "@/views/admin/fees/lib/format";

export function FundPicker({
  funds,
  selected,
  onSelect,
  policies,
}: {
  funds: { service: string; title: string }[];
  selected: string;
  onSelect: (service: string) => void;
  policies: Map<string, FeePolicy>;
}) {
  const t = useT();
  return (
    <StaggerItem className="flex flex-wrap gap-2">
      {funds.map((fund) => {
        const policy = policies.get(fund.service);
        const active = fund.service === selected;
        const pending = isPendingChange(policy?.pending?.state);
        return (
          <button
            key={fund.service}
            type="button"
            onClick={() => onSelect(fund.service)}
            aria-pressed={active}
            className={`rounded-lg border px-3 py-2 text-left text-sm transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring ${
              active ? "border-main-accent-t1 bg-main-accent-t1/10" : "border-border hover:bg-muted/50"
            }`}
          >
            <span className="block font-medium">{fund.title}</span>
            <span className="block text-xs text-muted-foreground">
              {policy?.configured ? `${pct(policy.management_bps)} / ${pct(policy.performance_bps)}` : t("admin.fees.noFee")}
            </span>
            {/* Said on the chip, not only on the card: a change on the way is what an
                operator scanning the row most needs to know before they pick a fund. */}
            {pending && <span className="block text-xs text-main-accent-t1">{t("admin.fees.pendingChip")}</span>}
          </button>
        );
      })}
    </StaggerItem>
  );
}
