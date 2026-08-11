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

// The back affordance is a bare icon with no border of its own, so it needs a rounded box
// for the focus ring to trace.
const BACK_FOCUS = "rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring";

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
        <Link href={backHref} aria-label="Back" className={cn("-ml-1 flex size-6 shrink-0 items-center justify-center text-foreground", BACK_FOCUS)}>
          <ChevronLeft className="size-6" />
        </Link>
      ) : (
        onBack && (
          <button type="button" onClick={onBack} aria-label="Back" className={cn("-ml-1 flex size-6 shrink-0 items-center justify-center text-foreground", BACK_FOCUS)}>
            <ChevronLeft className="size-6" />
          </button>
        )
      )}
      {/* Both sizes land on the type scale, so the pushed/root distinction is carried by
          size plus centring rather than the 3px that used to separate them. */}
      <h1 className={cn("min-w-0 flex-1 truncate font-semibold tracking-normal text-foreground", pushed ? "text-center text-base" : "text-lg")}>{title}</h1>
      {pushed && !right ? <span className="size-6 shrink-0" aria-hidden /> : right}
    </header>
  );
}
