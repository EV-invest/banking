"use client";

import { Loader2 } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, Input } from "@evinvest/uikit";

import type { AllocationWrite } from "@/entities/admin/api/admin-client";
import type { AllocationIcon } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { StaggerItem } from "@/shared/ui/motion";
import { IconSelect } from "@/views/admin/allocations/ui/pickers";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

export function RegisterForm({ busy, onCancel, onSubmit }: { busy: boolean; onCancel: () => void; onSubmit: (body: AllocationWrite) => void }) {
  const t = useT();
  const [service, setService] = useState("");
  const [title, setTitle] = useState("");
  const [summary, setSummary] = useState("");
  // Pre-selected rather than blank, and `fund` specifically because that is what the hub
  // stores when none is sent — so the form shows the outcome of leaving it alone.
  const [icon, setIcon] = useState<AllocationIcon>("fund");

  // Mirrors `ServiceId::parse` — the hub rejects anything else, so say so before the
  // round-trip rather than surfacing a validation error after it.
  const slugOk = /^[A-Za-z0-9_-]{1,64}$/.test(service);

  return (
    // A `StaggerItem` although it is not part of the page's own arrival: it mounts when
    // the operator asks for it, and the parent's variants carry it in the same way.
    <StaggerItem as={Card}>
      <CardContent className="space-y-4 py-6">
        <div className="grid gap-4 md:grid-cols-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">{t("admin.alloc.col.serviceId")}</span>
            {/* The two placeholders are format examples, not prose — a slug and a proper
                noun — so they stay as they are in every locale. */}
            <Input value={service} onChange={(e) => setService(e.target.value.trim())} placeholder="quy-nhon-fund" spellCheck={false} className="w-full font-mono-tech" />
            <span className={cn("text-xs", service && !slugOk ? "text-destructive" : "text-muted-foreground")}>
              {service && !slugOk ? t("admin.alloc.slugInvalid") : t("admin.alloc.slugHint")}
            </span>
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">{t("admin.alloc.field.title")}</span>
            <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Quy Nhon Fund" className="w-full" />
          </label>
          {/* Not a `<label>` — see the row editor: the trigger is a button, so a wrapping
              label would toggle the popup a second time. */}
          <div className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">{t("admin.alloc.field.icon")}</span>
            <IconSelect value={icon} onChange={setIcon} />
          </div>
          <label className="flex flex-col gap-1.5">
            <span className="text-sm text-muted-foreground">{t("admin.alloc.field.summary")}</span>
            <Input value={summary} onChange={(e) => setSummary(e.target.value)} placeholder={t("admin.alloc.placeholder.summary")} className="w-full" />
          </label>
        </div>
        <div className="flex items-center gap-3">
          <p className="min-w-0 text-xs text-muted-foreground">{t("admin.alloc.registerHint")}</p>
          {/* i18n-max: 12 per verb — both Buttons are `shrink-0` beside the hint above. */}
          <Button type="button" variant="outline" className="ml-auto" onClick={onCancel}>
            {t("ui.cancel")}
          </Button>
          <Button type="button" className={cn(TEAL_CTA)} disabled={busy || !slugOk || !title.trim()} onClick={() => onSubmit({ service, title, summary, icon })}>
            {busy ? <Loader2 className="size-4 animate-spin" /> : null}
            {t("admin.alloc.registerSubmit")}
          </Button>
        </div>
      </CardContent>
    </StaggerItem>
  );
}
