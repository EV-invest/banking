"use client";

// Small presentational primitives shared across the admin-console views: the page
// header (eyebrow · title · subtitle · environment badge · action slot), a status dot,
// and a switch toggle. Kept dependency-light (no uncertain uikit exports) — plain
// buttons + tokens, matching the wallet view's idiom.

import type { ReactNode } from "react";

import { cn } from "@/shared/lib/cn";
import { usePlatform } from "@/shared/lib/use-platform";
import { statusTone } from "@/views/admin/lib/format";

// Server-truthful environment (the BFF's APP_ENV); anything unrecognised — including
// the default "development" — reads as DEV.
const ENV_BADGES: Record<string, { label: string; tone: string }> = {
  production: { label: "PROD", tone: "text-main-accent-t2" },
  staging: { label: "STAGING", tone: "text-main-accent-t3" },
};

export function AdminHeader({ eyebrow, title, subtitle, action }: { eyebrow: string; title: string; subtitle: string; action?: ReactNode }) {
  const environment = usePlatform()?.environment;
  const badge = environment ? (ENV_BADGES[environment] ?? { label: "DEV", tone: "text-muted-foreground" }) : null;
  return (
    <header className="flex flex-wrap items-start justify-between gap-4">
      <div className="space-y-1">
        <p className="font-mono-tech text-xs uppercase tracking-widest text-main-accent-t1">{eyebrow}</p>
        <h1 className="font-sans text-2xl font-semibold text-foreground">{title}</h1>
        <p className="text-sm text-muted-foreground">{subtitle}</p>
      </div>
      <div className="flex items-center gap-3">
        {badge && (
          <span className={cn("inline-flex items-center gap-1.5 rounded-full border border-border px-2.5 py-1 text-xs font-medium", badge.tone)}>
            <span className="size-1.5 rounded-full bg-current" />
            {badge.label}
          </span>
        )}
        {action}
      </div>
    </header>
  );
}

export function StatusDot({ status, label }: { status: string; label?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-1.5 text-sm font-medium capitalize", statusTone(status))}>
      <span className="size-1.5 rounded-full bg-current" />
      {label ?? status}
    </span>
  );
}

export function Toggle({ on, onChange, disabled, label }: { on: boolean; onChange: (next: boolean) => void; disabled?: boolean; label?: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors disabled:opacity-50",
        on ? "bg-main-accent-t1" : "bg-muted",
      )}
    >
      <span className={cn("inline-block size-4 rounded-full bg-main-mist transition-transform", on ? "translate-x-[18px]" : "translate-x-0.5")} />
    </button>
  );
}
