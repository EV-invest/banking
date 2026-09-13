"use client";

// An on-chain destination: the rail, then the address.
//
// The rails on offer are the fund's configured payout rails — the same read the revenue
// screen's chips were built on — because an external order ships as a withdrawal and can
// only ship on a rail with a running watcher. A rail the hub does not list is not a
// choice the form can make on its behalf.

import { useId } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle, Input, Label, RadioGroup, RadioGroupItem, Skeleton } from "@evinvest/uikit";

import { fundRevenueResource } from "@/entities/admin/model/admin-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";
import { NetworkMark } from "@/shared/ui/icons/networks";
import { ResourceError } from "@/shared/ui/resource-error";
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
  const railId = useId();
  const revenue = useResource(fundRevenueResource);
  const rails = revenue.data?.rails ?? null;

  return (
    <div className="space-y-2">
      {!rails && revenue.error ? (
        <ResourceError error={revenue.error} onRetry={() => void revenue.refresh()} retrying={revenue.isValidating} />
      ) : !rails ? (
        <Skeleton className="h-9 w-full" />
      ) : rails.length === 0 ? (
        <Empty className="border p-4">
          <EmptyHeader>
            <EmptyTitle className="text-sm">{t("admin.payments.noRail")}</EmptyTitle>
            <EmptyDescription className="text-xs">{t("admin.payments.noRailHint")}</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button asChild size="sm" variant="outline">
              <Link href="/admin/treasury">{t("nav.treasury")}</Link>
            </Button>
          </EmptyContent>
        </Empty>
      ) : (
        // The uikit group rather than a hand-rolled `role="radiogroup"`: it brings roving
        // focus, so the rails are one tab stop and the arrow keys move between them.
        <RadioGroup value={network} onValueChange={(next) => onChange({ network: next })} aria-label={t("admin.rail")} className="flex flex-wrap gap-2">
          {rails.map((rail) => {
            const selected = rail.network === network;
            const id = `${railId}-${rail.network}`;
            return (
              <Label
                key={rail.network}
                htmlFor={id}
                className={cn(
                  "flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-2 text-xs font-medium transition-colors",
                  selected ? "border-primary bg-primary/10" : "border-border hover:bg-foreground/5",
                )}
              >
                <RadioGroupItem id={id} value={rail.network} />
                <NetworkMark network={rail.network} className="size-3.5 shrink-0" />
                {railLabel(rail.network, t)}
              </Label>
            );
          })}
        </RadioGroup>
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
