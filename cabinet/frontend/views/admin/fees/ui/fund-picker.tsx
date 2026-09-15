"use client";

// One chip per fund, with its rate inline so the operator can see at a glance which
// products are priced, which are still charging nothing, and which have a change on the way.

import { useT } from "@evinvest/i18n/react";
import { Toggle } from "@evinvest/uikit";

import type { FeePolicy } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
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
          <Toggle
            key={fund.service}
            variant="outline"
            pressed={active}
            // Pressing the selected fund again keeps it selected: the screen always shows one.
            onPressedChange={(pressed) => pressed && onSelect(fund.service)}
            className={cn("h-auto flex-col items-start gap-0 px-3 py-2 text-left text-sm", active && "border-primary bg-primary/10")}
          >
            <span className="block font-medium">{fund.title}</span>
            <span className="block text-xs tabular-nums text-ink-soft">
              {policy?.configured ? `${pct(policy.management_bps)} / ${pct(policy.performance_bps)}` : t("admin.fees.noFee")}
            </span>
            {/* Said on the chip, not only on the card: a change on the way is what an
                operator scanning the row most needs to know before they pick a fund. */}
            {pending && <span className="block text-xs text-accent-debug">{t("admin.fees.pendingChip")}</span>}
          </Toggle>
        );
      })}
    </StaggerItem>
  );
}
