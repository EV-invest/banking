"use client";

import { BadgeCheck, Home, LineChart, ListChecks, LogOut, Settings, type LucideIcon } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { type ReactNode, useEffect, useState } from "react";

import { Logo } from "@/application/layout/logo";
import { cn } from "@/shared/lib/cn";
import { csrfHeader } from "@/shared/lib/csrf-client";

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

// The signed-in app shell's left rail (Figma cabinet sidebar). Persistent across the
// `(app)` route group; auth is enforced upstream in `proxy.ts`.
export function Sidebar() {
  const pathname = usePathname();
  return (
    <aside className="sticky top-0 flex h-screen w-[248px] shrink-0 flex-col gap-7 overflow-y-auto border-r border-border bg-main-surface px-[18px] pb-5 pt-6">
      <Link href="/" aria-label="EV Investment — home" className="block">
        <Logo className="h-9 w-auto text-main-mist" />
      </Link>

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
      </nav>

      <div className="flex-1" />

      <div className="flex flex-col gap-3">
        <nav aria-label="Secondary">
          <NavLink item={{ href: "/settings", label: "Settings", icon: Settings, active: (p) => p.startsWith("/settings") }} active={pathname.startsWith("/settings")} />
        </nav>
        <div className="h-px w-full bg-border" />
        <AccountChip />
      </div>
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

// Account chip bound to the BFF session. Behind the auth gate this is always a real user;
// a dropped/stale session (cookie present, server-side session gone) bounces to /login.
function AccountChip() {
  const [email, setEmail] = useState<string | null | undefined>(undefined);

  useEffect(() => {
    let active = true;
    fetch("/api/auth/session")
      .then((r) => r.json() as Promise<{ authenticated: boolean; user?: { email: string } }>)
      .then((s) => {
        if (!active) return;
        if (s.authenticated && s.user) setEmail(s.user.email);
        else {
          // Cookie present but the server-side session is gone (e.g. a restart): the proxy
          // let us through on cookie presence, so bounce to a fresh sign-in.
          setEmail(null);
          window.location.href = "/login";
        }
      })
      .catch(() => {
        // Transient failure — keep the chip neutral; don't force a redirect on a blip.
        if (active) setEmail(null);
      });
    return () => {
      active = false;
    };
  }, []);

  async function signOut() {
    await fetch("/api/auth/logout", { method: "POST", headers: csrfHeader() });
    window.location.href = "/loggedout";
  }

  return (
    <div className="flex items-center gap-[10px] border-t border-border pt-[14px]">
      <span className="flex size-[34px] shrink-0 items-center justify-center rounded-full bg-main-accent-t1/15 text-xs font-semibold text-main-accent-t1">{initialsOf(email)}</span>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13px] font-semibold text-main-mist">{displayName(email)}</p>
        <p className="flex items-center gap-[5px] text-[11px] font-medium text-main-accent-t1">
          <BadgeCheck className="size-3" /> Verified
        </p>
      </div>
      <button type="button" onClick={signOut} aria-label="Sign out" className="text-muted-foreground transition-colors hover:text-foreground">
        <LogOut className="size-4" />
      </button>
    </div>
  );
}

function displayName(email: string | null | undefined): string {
  if (email === undefined) return "…";
  if (!email) return "Account";
  const handle = email.split("@")[0] ?? email;
  const parts = handle.split(/[._-]+/).filter(Boolean);
  const first = parts[0] ? cap(parts[0]) : handle;
  const last = parts[1] ? `${parts[1][0]!.toUpperCase()}.` : "";
  return [first, last].filter(Boolean).join(" ");
}

function initialsOf(email: string | null | undefined): string {
  if (!email) return "EV";
  const parts = (email.split("@")[0] ?? "").split(/[._-]+/).filter(Boolean);
  const a = parts[0]?.[0] ?? email[0] ?? "E";
  const b = parts[1]?.[0] ?? "";
  return (a + b).toUpperCase();
}

function cap(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
