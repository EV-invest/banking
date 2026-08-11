"use client";

import { ArrowUpFromLine, Bell, Boxes, Home, Landmark, LayoutGrid, LineChart, ListChecks, PanelsTopLeft, Receipt, Settings, UsersRound, Wallet, type LucideIcon } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { type ReactNode, useEffect, useState } from "react";

import { fetchAllocations } from "@/entities/fund/api/fund-client";
import { useUnreadCount, useUnreadCountPolling } from "@/entities/notification/model/notification-store";
import type { Allocation } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { useSession } from "@/shared/lib/use-session";
import { DUR, EASE } from "@/shared/ui/motion";

interface NavItem {
  href: string;
  label: string;
  icon: LucideIcon;
  active: (path: string) => boolean;
}

// FUND group — the primary surfaces. "Home" is the portfolio dashboard; "Wallet" is the
// balance plus deposit/withdraw/activity, which the dashboard's money actions also link
// into. The mobile tab bar has no room for a sixth tab, so there /wallet still lights up
// Operations.
const FUND: NavItem[] = [
  { href: "/", label: "Home", icon: Home, active: (p) => p === "/" },
  { href: "/invest", label: "Invest", icon: LineChart, active: (p) => p.startsWith("/invest") },
  { href: "/wallet", label: "Wallet", icon: Wallet, active: (p) => p.startsWith("/wallet") },
  { href: "/operations", label: "Operations", icon: ListChecks, active: (p) => p.startsWith("/operations") },
];

// PRODUCTS is the open allocation registry, not a fixed list: a fund appears in the rail
// because an operator registered and opened it. It used to name one product literally,
// which went stale the moment a second one was registered.
const PRODUCT_TONES = [
  "bg-main-accent-t1/15 text-main-accent-t1",
  "bg-main-accent-t2/15 text-main-accent-t2",
  "bg-main-accent-t3/15 text-main-accent-t3",
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
  { href: "/admin/allocations", label: "Allocations", icon: Boxes, active: (p) => p.startsWith("/admin/allocations") },
  { href: "/admin/valuation", label: "Valuation & redemptions", icon: Receipt, active: (p) => p.startsWith("/admin/valuation") },
];

// Every rail row is a hand-written Link, so keyboard focus rides on this string. It runs
// The ring is the solid token, never a tint: at 50% the teal composites to 1.9–2.3:1 against
// every surface in this theme, under the 3:1 SC 1.4.11 floor, where solid clears it
// everywhere. The offset keeps the ring legible around the active row, whose fill is that
// same teal.
const NAV_FOCUS = "outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-main-surface";

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
  // The rail is mounted on every signed-in screen, so it is the one place the unread
  // count is polled from — every other consumer reads the shared store.
  useUnreadCountPolling();
  const unread = useUnreadCount();
  const [products, setProducts] = useState<Allocation[]>([]);

  useEffect(() => {
    // A failed catalog read leaves the group empty rather than blocking the rail — the
    // nav is not the place to surface an API error.
    fetchAllocations()
      .then((list) => setProducts(list.allocations ?? []))
      .catch(() => setProducts([]));
  }, []);
  return (
    <aside className="flex h-full w-[var(--cabinet-rail-w)] flex-col gap-7 overflow-y-auto border-r border-border bg-main-surface px-4.5 pb-5 pt-6">
      <nav aria-label="Primary" className="flex flex-col gap-4.5">
        <Group label="Fund">
          {FUND.map((item) => (
            <NavLink key={item.label} item={item} active={item.active(pathname)} />
          ))}
        </Group>
        {products.length > 0 && (
          <Group label="Products">
            {products.map((p, i) => {
              // Every row pointed at `/invest`, so naming a product in the rail took you to
              // the list of all of them. The product's own page is keyed by its service id.
              const href = `/invest/${encodeURIComponent(p.service)}`;
              const active = pathname === href;
              return (
                <Link
                  key={p.service}
                  href={href}
                  aria-current={active ? "page" : undefined}
                  className={cn(
                    "flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors",
                    NAV_FOCUS,
                    active ? "bg-primary font-semibold text-primary-foreground" : "font-medium text-foreground hover:bg-foreground/5",
                  )}
                >
                  <span className={cn("flex size-5 shrink-0 items-center justify-center rounded-md text-xs font-semibold", PRODUCT_TONES[i % PRODUCT_TONES.length])}>
                    {p.title.charAt(0).toUpperCase()}
                  </span>
                  <span className="truncate">{p.title}</span>
                </Link>
              );
            })}
          </Group>
        )}
        {isAdmin && (
          <Group label="Administer">
            {ADMIN.map((item) => (
              <NavLink key={item.label} item={item} active={item.active(pathname)} />
            ))}
          </Group>
        )}
      </nav>

      <div className="flex-1" />

      <nav aria-label="Secondary" className="flex flex-col gap-1">
        <NavLink
          item={{ href: "/notifications", label: "Notifications", icon: Bell, active: (p) => p.startsWith("/notifications") }}
          active={pathname.startsWith("/notifications")}
          trailing={unread ? <UnreadPill count={unread} active={pathname.startsWith("/notifications")} /> : undefined}
        />
        <NavLink item={{ href: "/settings", label: "Settings", icon: Settings, active: (p) => p.startsWith("/settings") }} active={pathname.startsWith("/settings")} />
      </nav>
    </aside>
  );
}

function Group({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1 pl-1">
      <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{label}</p>
      {children}
    </div>
  );
}

// The active pill is one shared node, not a background class on each link: with a
// single `layoutId` across every NavLink in the rail, motion tracks it from the old
// link to the new one and slides it there, so the rail shows the move instead of
// blinking the highlight between rows. It spans both navs and the admin group
// because they share the id.
const ACTIVE_PILL = "cabinet-rail-active";

function NavLink({ item, active, trailing }: { item: NavItem; active: boolean; trailing?: ReactNode }) {
  const Icon = item.icon;
  const reduce = useReducedMotion();
  return (
    <Link
      href={item.href}
      aria-current={active ? "page" : undefined}
      className={cn(
        // `isolate` is load-bearing: it gives the link its own stacking context so
        // the pill's negative z-index stays behind the label and not behind the
        // rail's own background, which is where it would land otherwise.
        "relative isolate flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors",
        NAV_FOCUS,
        active ? "font-semibold text-primary-foreground" : "font-medium text-foreground hover:bg-foreground/5",
      )}
    >
      {active && (
        <motion.span
          // Dropping the id under reduced motion turns the slide into a cut while
          // leaving the highlight itself in place.
          layoutId={reduce ? undefined : ACTIVE_PILL}
          aria-hidden
          className="absolute inset-0 -z-10 rounded-lg bg-primary"
          transition={{ duration: DUR.base, ease: EASE.out }}
        />
      )}
      <Icon className="size-4.5" />
      <span className="flex-1">{item.label}</span>
      {trailing}
    </Link>
  );
}

// Capped at 99+ so a long-neglected inbox cannot widen the rail.
function UnreadPill({ count, active }: { count: number; active: boolean }) {
  return (
    <span
      aria-label={`${count} unread`}
      className={cn(
        "rounded-full px-2 py-0.5 text-xs font-semibold tabular-nums",
        active ? "bg-main-black text-foreground" : "bg-main-accent-t1/15 text-main-accent-t1",
      )}
    >
      {count > 99 ? "99+" : count}
    </span>
  );
}
