"use client";

// The two closed-vocabulary pickers the registry screen writes through: a product's icon
// and its default access level. Both share the same shape — narrow the uikit `Select`'s
// bare `string` back to the wire union by lookup, never by cast, so a typo in an option's
// `value` cannot reach the BFF as a 400 — and neither renders `SelectValue`, because both
// draw the raw stored wire word (`real_estate`, `view`) rather than a `SelectValue` that
// would have to be told what to say for it.

import type { Translate } from "@evinvest/i18n";
import { useT } from "@evinvest/i18n/react";

import { Select, SelectContent, SelectItem, SelectTrigger } from "@evinvest/uikit";

import type { AllocationAccessLevel, AllocationIcon } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { ALLOCATION_ICONS, ProductIcon } from "@/shared/ui/icons/products";
import { ACCESS_LEVELS, accessLabel } from "@/views/admin/lib/access";

const iconWords = (t: Translate): Record<AllocationIcon, string> => ({
  fund: t("admin.alloc.icon.fund", "Fund"),
  real_estate: t("admin.alloc.icon.real_estate", "Real estate"),
  trading: t("admin.alloc.icon.trading", "Trading"),
  yield: t("admin.alloc.icon.yield", "Yield"),
  venture: t("admin.alloc.icon.venture", "Venture"),
  treasury: t("admin.alloc.icon.treasury", "Treasury"),
  commodity: t("admin.alloc.icon.commodity", "Commodity"),
  credit: t("admin.alloc.icon.credit", "Credit"),
  index: t("admin.alloc.icon.index", "Index"),
  arbitrage: t("admin.alloc.icon.arbitrage", "Arbitrage"),
});

export function IconSelect({ value, onChange, className }: { value: AllocationIcon; onChange: (icon: AllocationIcon) => void; className?: string }) {
  const t = useT();
  const words = iconWords(t);
  return (
    <Select
      value={value}
      onValueChange={(v) => {
        const picked = ALLOCATION_ICONS.find((i) => i === v);
        if (picked) onChange(picked);
      }}
    >
      <SelectTrigger className={cn("w-full border-border bg-secondary", className)}>
        <span className="flex min-w-0 items-center gap-1.5">
          <ProductIcon icon={value} className="size-3.5 shrink-0" />
          <span className="truncate">{words[value]}</span>
        </span>
      </SelectTrigger>
      <SelectContent>
        {ALLOCATION_ICONS.map((icon) => (
          <SelectItem key={icon} value={icon}>
            <ProductIcon icon={icon} className="size-3.5 shrink-0" />
            {words[icon]}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

/** The product's default access — what a caller with no grant holds. Every level is
 *  offered, unlike the grant form's picker: an operator may drop a product back to
 *  `hidden`, which a grant itself may never carry. */
export function AccessSelect({ value, onChange, className }: { value: AllocationAccessLevel; onChange: (level: AllocationAccessLevel) => void; className?: string }) {
  const t = useT();
  return (
    <Select
      value={value}
      onValueChange={(v) => {
        const picked = ACCESS_LEVELS.find((l) => l === v);
        if (picked) onChange(picked);
      }}
    >
      <SelectTrigger className={cn("w-full border-border bg-secondary", className)}>
        <span className="truncate">{accessLabel(value, t)}</span>
      </SelectTrigger>
      <SelectContent>
        {ACCESS_LEVELS.map((level) => (
          <SelectItem key={level} value={level}>
            {accessLabel(level, t)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
