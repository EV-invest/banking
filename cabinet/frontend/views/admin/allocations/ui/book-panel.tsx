"use client";

// The per-product book panel: the secondary market's terms, and the switch that opens
// it. The same `Panel`-beside-the-table idiom as `GrantsPanel` and `IssuancePanel`.
//
// The policy is read through the investor route (`entities/book`) — the same read the
// product page uses to decide whether to offer "Trade" — so what the operator sees here
// is exactly what an investor's cabinet will act on. The write answers with the policy as
// saved, which is published straight into that cache: an operator who flips the switch
// and walks to the product page finds the control already there.

import { TriangleAlert } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { setBookPolicy } from "@/entities/admin/api/admin-client";
import { bookPolicyResource } from "@/entities/book/model/book-resource";
import type { Allocation } from "@/shared/contracts/admin";
import type { SetBookPolicyBody } from "@/shared/contracts/book";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { cn } from "@/shared/lib/cn";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { Settled } from "@/shared/ui/motion";
import { BookForm } from "@/views/admin/allocations/ui/book-form";
import { UnbackedAckBadge } from "@/views/admin/allocations/ui/book-unbacked-ack";
import { PanelHeader } from "@/views/admin/allocations/ui/panel-header";

export function BookPanel({ allocation, onClose, className }: { allocation: Allocation; onClose: () => void; className?: string }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const read = useResource(bookPolicyResource, allocation.service);
  const error = actionError ?? (read.data || !read.error ? null : errorMessage(read.error, t));

  const save = async (body: SetBookPolicyBody) => {
    setBusy(true);
    setActionError(null);
    setSaved(false);
    try {
      const policy = await setBookPolicy(body);
      bookPolicyResource.publish(policy, allocation.service);
      // Any other surface holding this product's terms — a terminal open in another tab
      // of the same session — re-reads on its own cadence; the tag names the fact.
      revalidateTag(TAG.bookPolicy);
      setSaved(true);
      return true;
    } catch (e) {
      setActionError(errorMessage(e, t));
      return false;
    } finally {
      setBusy(false);
    }
  };

  return (
    // Fixed width, matching `GrantsPanel` — see the note there. `className` lets the
    // bottom-sheet presentation widen it to the sheet instead.
    <Card className={cn("w-85", className)}>
      <CardContent className="space-y-5 py-5">
        <PanelHeader allocation={allocation} onClose={onClose} />

        {error && (
          <p className="flex items-center gap-2 text-xs text-destructive">
            <TriangleAlert className="size-3.5" /> {error}
          </p>
        )}

        <div className="space-y-2">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t("admin.alloc.book.title")}</p>
            <UnbackedAckBadge acknowledged={read.data?.allow_unbacked_trading === true} />
          </div>
          <Settled loading={!read.data && !read.error} skeleton={<Skeleton className="h-40 w-full" />}>
            {(read.data || read.error) && <BookForm key={read.data?.updated_at ?? "none"} allocation={allocation} policy={read.data ?? null} busy={busy} saved={saved} onSubmit={save} />}
          </Settled>
          <p className="text-xs text-muted-foreground">{t("admin.alloc.book.note")}</p>
        </div>
      </CardContent>
    </Card>
  );
}
