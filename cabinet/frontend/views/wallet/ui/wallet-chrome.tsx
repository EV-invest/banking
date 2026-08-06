"use client";

import { ChevronLeft } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

import { cn } from "@/shared/lib/cn";

// Shared chrome for the four wallet screens (Figma `cabinet/wallet/*` desktop +
// `cabinet/mobile/wallet/*`). The two viewports differ in kind, not degree: desktop gets a
// topbar with a title/subtitle column and trailing actions, mobile gets an iOS-style appbar
// with a back affordance — so both are authored explicitly rather than reflowed from one.
export function WalletScreen({ title, subtitle, back, actions, children }: { title: string; subtitle?: string; back?: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-col">
      <div className="flex items-center gap-3 border-b border-border bg-main-surface px-5 pb-3.5 pt-4 lg:hidden">
        {back && (
          <Link
            href={back}
            aria-label="Back"
            className="-m-1 shrink-0 rounded-md p-1 text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <ChevronLeft className="size-6" />
          </Link>
        )}
        <p className="truncate text-lg font-semibold text-foreground">{title}</p>
      </div>

      <div className="hidden items-center gap-4 px-8 pb-6 pt-6.5 lg:flex">
        <div className="min-w-0 flex-1">
          <h1 className="font-sans text-2xl font-semibold text-foreground">{title}</h1>
          {subtitle && <p className="text-sm text-muted-foreground">{subtitle}</p>}
        </div>
        {actions && <div className="flex shrink-0 gap-2.5">{actions}</div>}
      </div>

      <div className="flex flex-col gap-3.5 px-5 pb-6 pt-4.5 lg:gap-5 lg:px-8 lg:pb-8 lg:pt-0">{children}</div>
    </div>
  );
}

export const WALLET_CARD = "rounded-xl border border-border bg-main-card";
// The teal primary and the hairline-outlined secondary, shared by every wallet CTA. Both are
// hand-written rather than uikit Buttons, so the keyboard focus ring rides along here — every
// wallet CTA is a link or a button built from one of these two strings.
export const WALLET_CTA =
  "flex items-center justify-center rounded-lg bg-main-accent-t1 font-medium text-main-black outline-none transition-opacity hover:opacity-90 focus-visible:ring-2 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50";
export const WALLET_CTA_GHOST =
  "flex items-center justify-center rounded-lg border border-border font-medium text-foreground outline-none transition-colors hover:bg-foreground/5 focus-visible:ring-2 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50";

// The all-caps field/section label used across the wallet cards.
export function FieldLabel({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={cn("flex items-center gap-1.5 text-xs font-medium text-muted-foreground", className)}>{children}</span>;
}
