"use client";

import { ArrowUpFromLine, Home, Landmark, LayoutGrid, LineChart, ListChecks, PanelsTopLeft, Receipt, Settings, UsersRound, type LucideIcon } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";

import { cn } from "@/shared/lib/cn";
import { useSession } from "@/shared/lib/use-session";

interface NavItem {
  href: string;
  label: string;
  icon: LucideIcon;
  active: (path: string) => boolean;
}

// FUND group — the primary surfaces. "Home" is the portfolio dashboard; deposit/withdraw
// live behind the dashboard's money actions (matching the Figma cabinet, which has no
// standalone Wallet nav item).
const FUND: NavItem[] = [
  { href: "/", label: "Home", icon: Home, active: (p) => p === "/" },
  { href: "/invest", label: "Invest", icon: LineChart, active: (p) => p.startsWith("/invest") },
  { href: "/operations", label: "Operations", icon: ListChecks, active: (p) => p.startsWith("/operations") },
];

const PRODUCTS = [
  { href: "/invest", label: "Quy Nhon Fund", badge: "Q", tone: "bg-main-accent-t1/15 text-main-accent-t1" },
  { href: "/invest", label: "Coastal Yield", badge: "C", tone: "bg-main-accent-t2/15 text-main-accent-t2" },
];

// ADMINISTER group — the operator console. Rendered only for a non-investor session
// role (the BFF's `/api/auth/session` `isAdmin`); every screen is also authorized
// server-side, so hiding the nav is cosmetic, not the security boundary.
const ADMIN: NavItem[] = [
  { href: "/admin/overview", label: "Overview", icon: LayoutGrid, active: (p) => p.startsWith("/admin/overview") },
  { href: "/admin/users", label: "Users", icon: UsersRound, active: (p) => p.startsWith("/admin/users") },
  { href: "/admin/cabinet", label: "Cabinet", icon: PanelsTopLeft, active: (p) => p.startsWith("/admin/cabinet") },
  { href: "/admin/treasury", label: "Treasury", icon: Landmark, active: (p) => p.startsWith("/admin/treasury") },
  { href: "/admin/withdrawals", label: "Withdrawals", icon: ArrowUpFromLine, active: (p) => p.startsWith("/admin/withdrawals") },
  { href: "/admin/valuation", label: "Valuation & redemptions", icon: Receipt, active: (p) => p.startsWith("/admin/valuation") },
];

// The signed-in app shell's left rail (Figma cabinet sidebar). Persistent across the
// `(app)` route group; auth is enforced upstream in `proxy.ts`. Positioned by a
// `lg:fixed` wrapper in the `(app)` layout — below 1024px the sidebar is hidden,
// replaced by the fixed BottomNavbar. `overflow-y-auto` is a safety valve only: the
// rail scrolls internally solely when it can't fit (e.g. the admin nav on a short
// viewport), so nothing gets clipped.
export function Sidebar() {
  const pathname = usePathname();
  const session = useSession();
  const isAdmin = session?.user?.isAdmin ?? false;
  return (
    <aside className="flex h-full w-[var(--cabinet-rail-w)] flex-col gap-7 overflow-y-auto border-r border-border bg-main-surface px-[18px] pb-5 pt-6">
      <nav aria-label="Primary" className="flex flex-col gap-[18px]">
        <Group label="Fund">
          {FUND.map((item) => (
            <NavLink key={item.label} item={item} active={item.active(pathname)} />
          ))}
        </Group>
        <Group label="Products">
          {PRODUCTS.map((p) => (
            <Link
              key={p.label}
              href={p.href}
              className="flex items-center gap-[11px] rounded-lg px-3 py-2 text-[13.5px] font-medium text-main-mist/90 transition-colors hover:bg-foreground/[0.04]"
            >
              <span className={cn("flex size-5 items-center justify-center rounded-md text-[11px] font-semibold", p.tone)}>{p.badge}</span>
              {p.label}
            </Link>
          ))}
        </Group>
        {isAdmin && (
          <Group label="Administer">
            {ADMIN.map((item) => (
              <NavLink key={item.label} item={item} active={item.active(pathname)} />
            ))}
          </Group>
        )}
      </nav>

      <div className="flex-1" />

      <nav aria-label="Secondary">
        <NavLink item={{ href: "/settings", label: "Settings", icon: Settings, active: (p) => p.startsWith("/settings") }} active={pathname.startsWith("/settings")} />
      </nav>
    </aside>
  );
}

function Group({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1 pl-1">
      <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-main-mist/40">{label}</p>
      {children}
    </div>
  );
}

function NavLink({ item, active }: { item: NavItem; active: boolean }) {
  const Icon = item.icon;
  return (
    <Link
      href={item.href}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex items-center gap-[11px] rounded-lg px-3 py-[9px] text-sm transition-colors",
        active ? "bg-main-accent-t1 font-semibold text-main-black" : "font-medium text-main-mist/90 hover:bg-foreground/[0.04]",
      )}
    >
      <Icon className="size-[18px]" />
      {item.label}
    </Link>
  );
}
