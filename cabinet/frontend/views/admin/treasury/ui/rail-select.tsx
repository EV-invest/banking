"use client";

// The rail picker the treasury's two chain-proven forms share — recording an arrival and
// proposing a seed both name a transfer by the rail it landed on. Only rails the hub
// actually reported with a treasury address are offered: an address minted for a rail
// nothing watches is exactly the mistake these forms exist to prevent.

import { useT } from "@evinvest/i18n/react";
import { Select, SelectContent, SelectItem, SelectTrigger } from "@evinvest/uikit";

import type { RailLiquidity } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { NetworkMark } from "@/shared/ui/icons/networks";
import { railLabel } from "@/views/admin/lib/format";

/** The rails a form may name — those with a watched treasury address. */
export function watchedRails(rails: RailLiquidity[] | undefined): RailLiquidity[] {
  return rails?.filter((r) => r.treasury_address) ?? [];
}

export function RailSelect({ value, options, onChange }: { value: string; options: RailLiquidity[]; onChange: (network: string) => void }) {
  const t = useT();
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-sm text-ink-soft">{t("admin.rail")}</span>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger className="w-full border-border bg-secondary" disabled={options.length === 0}>
          {/* The placeholder is trigger text, not a selectable item — "Select a rail…"
              is not a rail. */}
          <span className={cn("flex min-w-0 items-center gap-1.5", !value && "text-ink-soft")}>
            {value && <NetworkMark network={value} className="size-3.5 shrink-0" />}
            <span className="truncate">{value ? railLabel(value, t) : options.length === 0 ? t("admin.treasury.noRailWithTreasury") : t("admin.treasury.selectRail")}</span>
          </span>
        </SelectTrigger>
        <SelectContent>
          {options.map((r) => (
            <SelectItem key={r.network} value={r.network}>
              <NetworkMark network={r.network} className="size-3.5 shrink-0" />
              {railLabel(r.network, t)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
