"use client";

// An on-chain destination: the rail, then the address.
//
// The rails on offer are the fund's configured payout rails — the same read the revenue
// screen's chips were built on — because an external order ships as a withdrawal and can
// only ship on a rail with a running watcher. A rail the hub does not list is not a
// choice the form can make on its behalf.

import { useT } from "@evinvest/i18n/react";
import { Input, Skeleton } from "@evinvest/uikit";

import { fundRevenueResource } from "@/entities/admin/model/admin-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { NetworkMark } from "@/shared/ui/icons/networks";
import { railLabel } from "@/views/admin/lib/format";

export function ExternalFields({
  network,
  address,
  onChange,
}: {
  network: string;
  address: string;
  onChange: (patch: { network?: string; address?: string }) => void;
}) {
  const t = useT();
  const revenue = useResource(fundRevenueResource);
  const rails = revenue.data?.rails ?? null;

  return (
    <div className="space-y-2">
      {!rails ? (
        <Skeleton className="h-10 w-full" />
      ) : rails.length === 0 ? (
        <p className="text-xs text-muted-foreground">{t("admin.payments.noRail")}</p>
      ) : (
        <div role="radiogroup" aria-label={t("admin.rail")} className="flex flex-wrap gap-2">
          {rails.map((rail) => {
            const selected = rail.network === network;
            return (
              <button
                key={rail.network}
                type="button"
                role="radio"
                aria-checked={selected}
                onClick={() => onChange({ network: rail.network })}
                className={cn(
                  "flex items-center gap-1.5 rounded-lg border px-3 py-2 text-xs font-medium outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring",
                  selected ? "border-primary bg-primary/10" : "border-border hover:bg-foreground/5",
                )}
              >
                <NetworkMark network={rail.network} className="size-3.5 shrink-0" />
                {railLabel(rail.network, t)}
              </button>
            );
          })}
        </div>
      )}
      <Input
        value={address}
        onChange={(e) => onChange({ address: e.target.value })}
        placeholder={t("admin.payments.placeholder.address")}
        aria-label={t("admin.payments.address")}
        spellCheck={false}
        className="font-mono-tech text-xs"
      />
    </div>
  );
}
