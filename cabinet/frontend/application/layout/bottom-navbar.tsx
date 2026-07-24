"use client";

import { Home, LayoutGrid, LineChart, ListChecks, Settings, type LucideIcon } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";

import { cn } from "@/shared/lib/cn";

interface TabItem {
  href: string;
  label: string;
  icon: LucideIcon;
  active: (path: string) => boolean;
}

// The 5-tab mobile navigation bar (Figma cabinet mobile tab bar). These tabs
// replace the desktop sidebar on narrow viewports (<1024px). "Products" maps
// to the invest page for now — both sidebar products ("Quy Nhon Fund", "Coastal
// Yield") resolve there. Active-tint uses the same teal accent as the sidebar's
// highlighted nav link for visual consistency.
const TABS: TabItem[] = [
  { href: "/", label: "Home", icon: Home, active: (p) => p === "/" },
  { href: "/invest", label: "Invest", icon: LineChart, active: (p) => p.startsWith("/invest") && !p.startsWith("/invest/products") },
  { href: "/operations", label: "Operations", icon: ListChecks, active: (p) => p.startsWith("/operations") },
  { href: "/invest", label: "Products", icon: LayoutGrid, active: (p) => p.startsWith("/invest") },
  { href: "/settings", label: "Settings", icon: Settings, active: (p) => p.startsWith("/settings") },
];

export function BottomNavbar() {
  const pathname = usePathname();

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
              "flex flex-1 flex-col items-center justify-center gap-0.5 rounded-lg py-1 text-[10px] font-medium transition-colors",
              isActive
                ? "text-main-accent-t1"
                : "text-main-mist/60 hover:text-main-mist/90",
            )}
          >
            <Icon className="size-5" />
            {tab.label}
          </Link>
        );
      })}
    </nav>
  );
}
