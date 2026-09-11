"use client";

import { Loader2 } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Select, SelectContent, SelectItem, SelectTrigger } from "@evinvest/uikit";

import type { AllocationGrantLevel } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { GRANT_LEVELS, accessLabel } from "@/views/admin/allocations/lib/access";
import { UserPicker, type PickedUser } from "@/views/admin/allocations/ui/user-picker";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

/** Raise one investor above the product's default. Only `view` and `invest` are offered —
 *  a grant may never carry `hidden`, which lives on the row's own access picker instead. */
export function GrantForm({ busy, onSubmit }: { busy: boolean; onSubmit: (userId: string, level: AllocationGrantLevel) => void }) {
  const t = useT();
  const [user, setUser] = useState<PickedUser | null>(null);
  const [level, setLevel] = useState<AllocationGrantLevel>("invest");

  return (
    <div className="space-y-3 rounded-lg border border-border bg-main-surface p-3">
      <div className="grid gap-2.5">
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">{t("admin.alloc.grants.field.user")}</span>
          <UserPicker value={user} onPick={setUser} />
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">{t("admin.alloc.grants.field.level")}</span>
          <Select value={level} onValueChange={(v) => setLevel(GRANT_LEVELS.find((l) => l === v) ?? level)}>
            <SelectTrigger className="w-full border-border bg-main-surface">
              <span className="truncate">{accessLabel(level, t)}</span>
            </SelectTrigger>
            <SelectContent>
              {GRANT_LEVELS.map((l) => (
                <SelectItem key={l} value={l}>
                  {accessLabel(l, t)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
      </div>
      <Button
        type="button"
        className={cn("w-full", TEAL_CTA)}
        disabled={busy || !user}
        onClick={() => user && onSubmit(user.userId, level)}
      >
        {busy ? <Loader2 className="size-4 animate-spin" /> : null}
        {t("admin.alloc.grants.submit")}
      </Button>
    </div>
  );
}
