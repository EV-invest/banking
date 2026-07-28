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
          <Link href={back} aria-label="Back" className="-m-1 shrink-0 rounded-md p-1 text-main-mist transition-colors hover:text-white">
            <ChevronLeft className="size-6" />
          </Link>
        )}
        <p className="truncate text-[18px] font-semibold text-foreground">{title}</p>
      </div>

      <div className="hidden items-center gap-4 px-8 pb-6 pt-[26px] lg:flex">
        <div className="min-w-0 flex-1">
          <h1 className="font-sans text-2xl font-semibold text-white">{title}</h1>
          {subtitle && <p className="text-[13px] text-main-mist/55">{subtitle}</p>}
        </div>
        {actions && <div className="flex shrink-0 gap-2.5">{actions}</div>}
      </div>

      <div className="flex flex-col gap-[14px] px-5 pb-6 pt-[18px] lg:gap-5 lg:px-8 lg:pb-8 lg:pt-0">{children}</div>
    </div>
  );
}

export const WALLET_CARD = "rounded-[14px] border border-border bg-main-card";
// The teal primary and the hairline-outlined secondary, shared by every wallet CTA.
export const WALLET_CTA = "flex items-center justify-center rounded-lg bg-main-accent-t1 font-medium text-main-black transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-50";
export const WALLET_CTA_GHOST = "flex items-center justify-center rounded-lg border border-border font-medium text-foreground transition-colors hover:bg-foreground/[0.04] disabled:pointer-events-none disabled:opacity-50";

// The 11px all-caps field/section label used across the wallet cards.
export function FieldLabel({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={cn("flex items-center gap-1.5 text-[11px] font-medium text-muted-foreground", className)}>{children}</span>;
}
