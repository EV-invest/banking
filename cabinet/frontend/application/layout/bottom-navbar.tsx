"use client";

import { Home, LayoutGrid, LineChart, ListChecks, Settings, type LucideIcon } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { cn } from "@/shared/lib/cn";
import { DUR, EASE } from "@/shared/ui/motion";

interface TabItem {
  href: string;
  label: string;
  icon: LucideIcon;
  active: (path: string) => boolean;
}

// The 5-tab mobile navigation bar (Figma cabinet mobile tab bar). These tabs
// replace the desktop sidebar on narrow viewports (<1024px). "Products" maps
// to the invest page for now — the sidebar product ("Quy Nhon Fund") resolves
// there. Active-tint uses the same teal accent as the sidebar's highlighted nav
// link for visual consistency.
const TABS: TabItem[] = [
  { href: "/", label: "Home", icon: Home, active: (p) => p === "/" },
  { href: "/invest", label: "Invest", icon: LineChart, active: (p) => p.startsWith("/invest") && !p.startsWith("/invest/products") },
  { href: "/operations", label: "Operations", icon: ListChecks, active: (p) => p.startsWith("/operations") || p.startsWith("/wallet") },
  { href: "/invest", label: "Products", icon: LayoutGrid, active: (p) => p.startsWith("/invest") },
  { href: "/settings", label: "Settings", icon: Settings, active: (p) => p.startsWith("/settings") },
];

// A shared `layoutId` marker, so switching tabs slides the rule across the bar
// rather than blinking it out and in. Two tabs resolve to /invest, so the id is
// keyed per tab label — not per href — or the marker would have nowhere to move to.
const ACTIVE_MARKER = "cabinet-tab-active";

export function BottomNavbar() {
  const pathname = usePathname();
  const reduce = useReducedMotion();

  return (
    <nav className="fixed bottom-0 left-0 right-0 z-50 flex h-[var(--cabinet-bottom-nav-h,64px)] items-center border-t border-border bg-main-surface px-2 pb-[env(safe-area-inset-bottom,0px)] lg:hidden">
      {TABS.map((tab) => {
        const Icon = tab.icon;
        const isActive = tab.active(pathname);
        return (
          <Link
            key={tab.label}
            href={tab.href}
            className={cn(
              "relative flex flex-1 flex-col items-center justify-center gap-0.5 rounded-lg py-1 text-xs font-medium transition-colors",
              // The offset is what earns its keep here: the active tab's fill is the same
              // teal as the ring, so without a gap the ring reads as the pill growing.
              "outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-main-surface",
              isActive ? "text-main-accent-t1" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {isActive && (
              <motion.span
                layoutId={reduce ? undefined : ACTIVE_MARKER}
                aria-hidden
                className="absolute inset-x-4 top-0 h-0.5 rounded-full bg-main-accent-t1"
                transition={{ duration: DUR.base, ease: EASE.out }}
              />
            )}
            <Icon className="size-5" />
            {tab.label}
          </Link>
        );
      })}
    </nav>
  );
}
