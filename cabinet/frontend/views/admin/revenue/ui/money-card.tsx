"use client";

import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { formatUsd } from "@/views/admin/lib/format";

export function MoneyCard({ label, value, hint, loading, emphasis }: { label: string; value: string | undefined; hint: string; loading: boolean; emphasis?: boolean }) {
  return (
    <Card>
      <CardContent className="space-y-1 py-5">
        <p className="text-xs text-muted-foreground">{label}</p>
        {loading ? (
          <Skeleton className="mt-1 h-8 w-28" />
        ) : (
          // One step for every figure; the payable one carries the difference in colour,
          // not in size, so the row keeps a single baseline.
          <p className={emphasis ? "text-3xl font-semibold tabular-nums text-main-accent-t2" : "text-3xl font-semibold tabular-nums"}>{formatUsd(value)}</p>
        )}
        {!loading && <p className="text-xs text-main-accent-t2">{hint}</p>}
      </CardContent>
    </Card>
  );
}
