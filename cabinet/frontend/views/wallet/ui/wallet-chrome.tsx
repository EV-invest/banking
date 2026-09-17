"use client";

import { useT } from "@evinvest/i18n/react";
import { ChevronLeft } from "lucide-react";
import { Link } from "@/shared/ui/cabinet-link";
import type { ReactNode } from "react";

import { cn } from "@/shared/lib/cn";
import { Reveal, SECTION_STAGGER, Stagger } from "@/shared/ui/motion";

// Shared chrome for the four wallet screens (Figma `cabinet/wallet/*` desktop +
// `cabinet/mobile/wallet/*`). The two viewports differ in kind, not degree: desktop gets a
// topbar with a title/subtitle column and trailing actions, mobile gets an iOS-style appbar
// with a back affordance — so both are authored explicitly rather than reflowed from one.
//
// The entrance lives here too, so all four wallet screens arrive the same way and a new one
// gets it for free. The title bar leads and the content column follows one step behind it —
// the two bars are mutually exclusive by breakpoint, so only ever one of them plays. Each
// screen supplies its own sections as `StaggerItem`s; anything passed as a bare element
// still renders, it just arrives with the column rather than in sequence.
export function WalletScreen({ title, subtitle, back, actions, children }: { title: string; subtitle?: string; back?: `/${string}`; actions?: ReactNode; children: ReactNode }) {
  const t = useT();
  return (
    <div className="flex flex-col">
      <Reveal className="flex items-center gap-3 border-b border-border bg-secondary px-5 pb-3.5 pt-4 lg:hidden">
        {back && (
          <Link
            href={back}
            aria-label={t("ui.back")}
            className="-m-1 shrink-0 rounded-md p-1 text-ink-soft outline-none transition-colors hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
          >
            <ChevronLeft className="size-6" />
          </Link>
        )}
        <p className="truncate text-lg font-semibold text-ink">{title}</p>
      </Reveal>

      <Reveal className="hidden items-center gap-4 px-8 pb-6 pt-6.5 lg:flex">
        <div className="min-w-0 flex-1">
          <h1 className="text-2xl font-semibold text-ink">{title}</h1>
          {subtitle && <p className="text-sm text-ink-soft">{subtitle}</p>}
        </div>
        {actions && <div className="flex shrink-0 gap-2.5">{actions}</div>}
      </Reveal>

      <Stagger delay={SECTION_STAGGER} step={SECTION_STAGGER} className="flex flex-col gap-3.5 px-5 pb-6 pt-4.5 lg:gap-5 lg:px-8 lg:pb-8 lg:pt-0">
        {children}
      </Stagger>
    </div>
  );
}

export const WALLET_CARD = "rounded-xl border border-border bg-card";

// The all-caps field/section label used across the wallet cards. With `htmlFor` it is a
// real label pinned to one control; a `<label>` wrapped around the label text, a tip button
// and the input binds to the FIRST labelable element — the tip — and leaves the input
// with no accessible name (the placeholder gets read instead).
export function FieldLabel({ children, className, htmlFor }: { children: ReactNode; className?: string; htmlFor?: string }) {
  const cls = cn("flex items-center gap-1.5 text-xs font-medium text-ink-soft", className);
  if (htmlFor) {
    return (
      <label htmlFor={htmlFor} className={cls}>
        {children}
      </label>
    );
  }
  return <span className={cls}>{children}</span>;
}
