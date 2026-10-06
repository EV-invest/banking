"use client";

import { TriangleAlert } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { setRailFrozen } from "@/entities/admin/api/admin-client";
import { railsResource } from "@/entities/admin/model/admin-resource";
import { errorMessage } from "@/shared/lib/api-client";
import { networkLabel } from "@/shared/lib/rail";
import { useResource } from "@/shared/lib/resource";
import { StaggerItem } from "@/shared/ui/motion";
import { AdminHeader, AdminScreen, Toggle } from "@/views/admin/ui/shell";

export function CoinsView() {
  const t = useT();
  const [writeError, setWriteError] = useState<string | null>(null);
  const read = useResource(railsResource);
  const rails = read.data ?? null;
  const error = writeError ?? (rails || !read.error ? null : errorMessage(read.error, t));

  const setLive = async (network: string, live: boolean) => {
    try {
      railsResource.publish(await setRailFrozen(network, !live));
    } catch (e) {
      setWriteError(errorMessage(e, t));
    }
  };

  return (
    <AdminScreen className="space-y-8">
      <AdminHeader eyebrow={t("admin.eyebrow.administer", "Administer")} title={t("nav.coins", "Coins")} subtitle={t("admin.coins.subtitle", "Rails users can deposit and withdraw on")} />

      {error && (
        <StaggerItem as="p" className="flex items-center gap-2 text-sm text-accent-error">
          <TriangleAlert className="size-4" /> {error}
        </StaggerItem>
      )}

      <StaggerItem as={Card}>
        <CardContent className="py-5">
          {!rails ? (
            <Skeleton className="h-40 w-full" />
          ) : (
            <div className="divide-y divide-border">
              {rails.map((r) => (
                <div key={r.network} className="flex items-center justify-between gap-3 py-3">
                  <div className="min-w-0">
                    <p className="text-sm font-medium">
                      {networkLabel(r.network)} <span className="text-xs text-ink-soft">· {t("admin.coins.gas", "gas in {coin}", { coin: r.gas_coin })}</span>
                    </p>
                    <p className="text-xs text-ink-soft">{!r.configured ? t("admin.coins.unconfigured", "Not run by this deployment") : r.frozen ? t("admin.coins.frozen", "Frozen — hidden from users, queued withdrawals held, no gas alerts") : t("admin.coins.live", "Live — deposits and withdrawals open")}</p>
                  </div>
                  <Toggle on={r.configured && !r.frozen} disabled={!r.configured} onChange={(live) => setLive(r.network, live)} label={networkLabel(r.network)} />
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </StaggerItem>
    </AdminScreen>
  );
}
