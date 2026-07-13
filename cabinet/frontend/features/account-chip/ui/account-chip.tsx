"use client";

import { BadgeCheck, LogOut } from "lucide-react";
import { useEffect, useState } from "react";

import { useProfile, useProfileSettled } from "@/entities/user/model/profile-store";
import { withBasePath } from "@/shared/config/base-path";
import { cn } from "@/shared/lib/cn";
import { csrfHeader } from "@/shared/lib/csrf-client";
import { SESSION_UNAVAILABLE, useSession } from "@/shared/lib/use-session";

import { clearIdentity, readIdentity, writeIdentity } from "../model/identity-cache";

// The account chip, rendered as a cabinet microfrontend inside the conductor's shared
// header (registered in site_conductor's mfe-registry as `cabinet.account`). It replaces
// the header's old Investor Portal button and owns all three states itself:
//   • loading      → a compact skeleton
//   • authenticated → avatar + name + Verified + sign-out
//   • signed-out   → the Investor Portal CTA (so anonymous marketing-site visitors still
//                     get a way into the cabinet)
//
// Framework-agnostic on purpose: the bundle mounts as a vanilla-React custom element on
// the CONDUCTOR origin, which has no cabinet Next router — so no next/link or
// next/navigation. Every destination is a cross-zone hard <a href> (PATTERNS §9), routed
// back into the zone via withBasePath(). And unlike the sidebar chip it NEVER redirects on
// a dropped session: an anonymous visitor on the public site must not be bounced to login.
export function AccountChip({ className }: { className?: string }) {
  const session = useSession();
  // Read once, at mount: the optimistic identity from the last visit.
  const [identity] = useState(readIdentity);
  const email = session?.user?.email ?? null;

  // Invalidate the cache only on a GENUINE signed-out result. A transient
  // session-endpoint blip resolves to the frozen SESSION_UNAVAILABLE sentinel — treating
  // that as sign-out would wipe the optimization and flash the CTA at a still-logged-in
  // user on the next load.
  const signedOut =
    session !== null && !session.authenticated && session !== SESSION_UNAVAILABLE;
  useEffect(() => {
    if (signedOut) clearIdentity();
  }, [signedOut]);

  // Still loading: trust the cache if we have one (same tab ⇒ same user in practice) so
  // the chip paints its final form now — but don't persist from here, the email isn't
  // confirmed yet. Never the email-derived name.
  if (session === null) {
    return identity ? (
      <AuthedChip
        className={className}
        email={identity.email}
        seedName={identity.name}
        persist={false}
      />
    ) : (
      <ChipSkeleton className={className} />
    );
  }
  if (!session.authenticated) return <SignInCta className={className} />;
  // Confirmed session: trust the seeded name only if it belongs to THIS account, so a
  // same-tab account switch never paints (or re-persists) the previous user's name.
  const seedName = identity?.email === email ? identity.name : null;
  return <AuthedChip className={className} email={email} seedName={seedName} persist />;
}

function AuthedChip({
  className,
  email,
  seedName,
  persist,
}: {
  className?: string;
  email: string | null;
  seedName: string | null;
  persist: boolean;
}) {
  // The profile refines the label (preferred/legal name); it only fetches here, in the
  // authenticated branch.
  const profile = useProfile();
  const settled = useProfileSettled();
  const realName = profile?.preferred_name || profile?.legal_name || null;
  // Resolution order: live profile name → the name seeded from last visit → (only once the
  // fetch has settled without a preferred/legal name) the email heuristic. Showing the
  // email-derived label while the profile is still loading is the flicker we avoid — it
  // would visibly correct itself a beat later. Until a name is known, render a skeleton,
  // not a name we'll replace.
  const name = realName ?? seedName ?? (settled ? displayName(email) : null);

  useEffect(() => {
    // Persist only from a confirmed session (persist) and only real names, so the cache
    // never holds a heuristic or a name that disagrees with its email.
    if (persist && realName) writeIdentity({ email, name: realName });
  }, [persist, email, realName]);

  async function signOut() {
    // Shell-owned logout (site-root /api/auth): revokes the shared session and clears
    // its cookies for every zone at once.
    await fetch("/api/auth/logout", { method: "POST", headers: csrfHeader() });
    window.location.href = withBasePath("/loggedout");
  }

  return (
    <div className={cn("flex items-center gap-2", className)}>
      <a
        href={withBasePath("/profile")}
        className="flex min-w-0 items-center gap-[10px] rounded-lg px-1.5 py-1 transition-colors hover:bg-foreground/[0.04]"
      >
        <span className="flex size-[34px] shrink-0 items-center justify-center rounded-full bg-main-accent-t1/15 text-xs font-semibold text-main-accent-t1">
          {initialsOf(email)}
        </span>
        <div className="min-w-0">
          {name ? (
            <p className="truncate text-[13px] font-semibold text-main-mist">{name}</p>
          ) : (
            <span className="my-[3px] block h-3 w-24 animate-pulse rounded bg-foreground/10" aria-hidden />
          )}
          <p className="flex items-center gap-[5px] text-[11px] font-medium text-main-accent-t1">
            <BadgeCheck className="size-3" /> Verified
          </p>
        </div>
      </a>
      <button
        type="button"
        onClick={signOut}
        aria-label="Sign out"
        className="shrink-0 text-muted-foreground transition-colors hover:text-foreground"
      >
        <LogOut className="size-4" />
      </button>
    </div>
  );
}

// Signed-out (or BFF-unavailable) state — the Investor Portal CTA the chip supersedes.
// Styled to match the conductor's old InvestorPortalButton (uikit outline) without pulling
// the uikit Button into the bundle. Links into the cabinet zone's sign-in.
function SignInCta({ className }: { className?: string }) {
  return (
    <a
      href={withBasePath("/login")}
      className={cn(
        "inline-flex h-9 items-center justify-center rounded-md border border-main-accent-t1 bg-transparent px-4 font-mono-tech text-xs tracking-wider text-main-accent-t1 transition-all duration-300 hover:bg-main-accent-t1 hover:text-main-black",
        className,
      )}
    >
      Investor Portal
    </a>
  );
}

function ChipSkeleton({ className }: { className?: string }) {
  return (
    <div className={cn("flex items-center gap-[10px] px-1.5 py-1", className)} aria-hidden>
      <span className="size-[34px] shrink-0 animate-pulse rounded-full bg-foreground/10" />
      <span className="h-3 w-20 animate-pulse rounded bg-foreground/10" />
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
