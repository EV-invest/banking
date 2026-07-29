import { ChevronRight, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/shared/lib/cn";

// The card + row vocabulary the Settings and Profile surfaces are built from
// (Figma `cabinet/mobile/settings` node 498:259 and `cabinet/mobile/profile` node
// 503:266): a 14px card on `main-card`, a compact section title, and rows split by
// hairlines. Section titles carry `font-sans` explicitly — the global `h1,h2,h3`
// rule swaps in the serif display face, which these headings are not.

export const CARD = "rounded-[14px] border border-border bg-main-card";

export function ListCard({ className, children }: { className?: string; children: ReactNode }) {
  return <section className={cn(CARD, "flex w-full min-w-0 flex-col px-4 pb-1.5 pt-1", className)}>{children}</section>;
}

export function ListCardTitle({ sub, children }: { sub?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-px pb-2 pt-3">
      <h2 className="font-sans text-[14px] font-semibold tracking-normal text-foreground">{children}</h2>
      {sub && <p className="text-[11px] font-medium text-muted-foreground">{sub}</p>}
    </div>
  );
}

export function Hairline() {
  return <div className="h-px shrink-0 bg-border" />;
}

export function Row({ className, children }: { className?: string; children: ReactNode }) {
  return <div className={cn("flex min-w-0 items-center justify-between gap-3 py-[13px]", className)}>{children}</div>;
}

/** Row leading block: a 14px title with an optional 11px caption underneath. */
export function RowLabel({ title, sub }: { title: ReactNode; sub?: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
      <span className="text-[14px] font-medium text-foreground">{title}</span>
      {sub && <span className="text-[11px] leading-snug text-muted-foreground">{sub}</span>}
    </div>
  );
}

/** Row trailing value — muted, right-aligned, truncated rather than pushing the row wide. */
export function RowValue({ className, children }: { className?: string; children: ReactNode }) {
  return <span className={cn("min-w-0 truncate text-right text-[13px] text-muted-foreground", className)}>{children}</span>;
}

/** A stacked label-over-value row (Figma `card-Personal`). */
export function StackRow({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1 py-3">
      <span className="text-[12px] font-medium text-muted-foreground">{label}</span>
      {children}
    </div>
  );
}

export function Chevron({ className }: { className?: string }) {
  return <ChevronRight className={cn("size-4 shrink-0 text-muted-foreground", className)} aria-hidden />;
}

const PILL_TONE = {
  positive: "bg-main-accent-t1/15 text-main-accent-t1",
  pending: "bg-main-accent-t3/15 text-main-accent-t3",
  neutral: "bg-foreground/[0.07] text-muted-foreground",
} as const;

export type PillTone = keyof typeof PILL_TONE;

export function Pill({ tone = "positive", icon: Icon, className, children }: { tone?: PillTone; icon?: LucideIcon; className?: string; children: ReactNode }) {
  return (
    <span className={cn("inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-[3px] text-[11px] font-semibold", PILL_TONE[tone], className)}>
      {Icon && <Icon className="size-[11px]" aria-hidden />}
      {children}
    </span>
  );
}

/** The teal initials disc used for the account avatar; the caller sizes it. */
export function InitialsAvatar({ initials, className }: { initials: string; className?: string }) {
  return (
    <span className={cn("flex shrink-0 items-center justify-center rounded-full bg-main-accent-t1 font-semibold text-main-black", className)} aria-hidden>
      {initials}
    </span>
  );
}
