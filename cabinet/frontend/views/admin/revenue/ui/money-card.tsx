"use client";

import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { formatUsd } from "@/views/admin/lib/format";

/** `unavailable` is the read-failed state: a muted dash, never a formatted `$0.00` — a
 *  zero the plane never reported would be read as a real balance. */
export function MoneyCard({ label, value, hint, loading, unavailable, emphasis }: { label: string; value: string | undefined; hint: string; loading: boolean; unavailable?: boolean; emphasis?: boolean }) {
  return (
    <Card>
      <CardContent className="space-y-1 py-5">
        <p className="text-xs text-muted-foreground">{label}</p>
        {loading ? (
          <Skeleton className="mt-1 h-8 w-28" />
        ) : unavailable ? (
          <p className="text-3xl font-semibold tabular-nums text-muted-foreground">—</p>
        ) : (
          // One step for every figure; the payable one carries the difference in colour,
          // not in size, so the row keeps a single baseline.
          <p className={emphasis ? "text-3xl font-semibold tabular-nums text-main-accent-t2" : "text-3xl font-semibold tabular-nums"}>{formatUsd(value)}</p>
        )}
        {!loading && !unavailable && <p className="text-xs text-main-accent-t2">{hint}</p>}
      </CardContent>
    </Card>
  );
}
