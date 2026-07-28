"use client";

import { ChevronLeft } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

import { cn } from "@/shared/lib/cn";

// The mobile app bar (Figma `cabinet/mobile/settings` node 498:260 and
// `cabinet/mobile/profile` node 503:267). Two shapes from one component: a root
// screen puts the title flush left with an optional trailing action, a pushed
// screen centres the title between a back affordance and a matching spacer.
// Hidden from `lg` up, where the desktop shell renders its own page heading.

export function MobileAppBar({
  title,
  backHref,
  onBack,
  right,
}: {
  title: string;
  backHref?: string;
  onBack?: () => void;
  right?: ReactNode;
}) {
  const pushed = !!backHref || !!onBack;
  return (
    <header className="sticky top-0 z-30 flex items-center gap-2 border-b border-border bg-main-surface px-5 pb-3.5 pt-4 lg:hidden">
      {backHref ? (
        <Link href={backHref} aria-label="Back" className="-ml-1 flex size-[22px] shrink-0 items-center justify-center text-foreground">
          <ChevronLeft className="size-[22px]" />
        </Link>
      ) : (
        onBack && (
          <button type="button" onClick={onBack} aria-label="Back" className="-ml-1 flex size-[22px] shrink-0 items-center justify-center text-foreground">
            <ChevronLeft className="size-[22px]" />
          </button>
        )
      )}
      <h1 className={cn("min-w-0 flex-1 truncate font-sans font-semibold tracking-normal text-foreground", pushed ? "text-center text-[17px]" : "text-[20px]")}>{title}</h1>
      {pushed && !right ? <span className="size-[22px] shrink-0" aria-hidden /> : right}
    </header>
  );
}
