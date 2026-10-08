"use client";

// One chip per fund, with its rate inline so the operator can see at a glance which
// products are priced, which are still charging nothing, and which have a change on the way.

import { useT } from "@evinvest/i18n/react";
import { Toggle } from "@evinvest/uikit";

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
          <Toggle
            key={fund.service}
            variant="outline"
            pressed={active}
            // Pressing the selected fund again keeps it selected: the screen always shows one.
            onPressedChange={(pressed) => pressed && onSelect(fund.service)}
            // The selected chip keeps a dark surface under its own `data-[state=on]:` colours:
            // the kit's pressed fill (uikit 0.27, primary) would put the teal "Change pending"
            // and the rate line on teal. Same variant, so tailwind-merge drops the kit's three.
            className="h-auto flex-col items-start gap-0 px-3 py-2 text-left text-sm data-[state=on]:border-primary-ink data-[state=on]:bg-hover data-[state=on]:text-ink"
          >
            <span className="block font-medium">{fund.title}</span>
            <span className="block text-xs tabular-nums text-ink-soft">
              {policy?.configured ? `${pct(policy.management_bps)} / ${pct(policy.performance_bps)}` : t("admin.fees.noFee", "No fee")}
            </span>
            {/* Said on the chip, not only on the card: a change on the way is what an
                operator scanning the row most needs to know before they pick a fund. */}
            {pending && <span className="block text-xs text-accent-debug">{t("admin.fees.pendingChip", "Change pending")}</span>}
          </Toggle>
        );
      })}
    </StaggerItem>
  );
}
