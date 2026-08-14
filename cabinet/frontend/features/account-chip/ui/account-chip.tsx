"use client";

import { BadgeCheck, LogOut } from "lucide-react";
import { useEffect, useState } from "react";

import { profileResource } from "@/entities/user/model/profile-resource";
import { withBasePath } from "@/shared/config/base-path";
import { cn } from "@/shared/lib/cn";
import { csrfHeader } from "@/shared/lib/csrf-client";
import { clearResources, useResource } from "@/shared/lib/resource";
import { SESSION_UNAVAILABLE, useSession } from "@/shared/lib/use-session";

import { clearIdentity, readIdentity, writeIdentity } from "../model/identity-cache";

// The chip's three controls are hand-written so the bundle stays free of the uikit Button,
// which means the keyboard focus ring has to be written out too.
const CHIP_FOCUS = "outline-none focus-visible:ring-2 focus-visible:ring-ring";

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
  // `isLoading` is false once the read has settled either way, so a profile that failed to
  // load falls through to the heuristic below instead of holding the skeleton forever.
  const { data: profile, isLoading } = useResource(profileResource);
  const settled = !isLoading;
  const realName = profile?.preferred_name || profile?.legal_name || null;
  // Resolution order: live profile name → the name seeded from last visit → (only once the
  // fetch has settled without a preferred/legal name) the email heuristic. Showing the
  // email-derived label while the profile is still loading is the flicker we avoid — it
  // would visibly correct itself a beat later. Until a name is known, render a skeleton,
  // not a name we'll replace. Truncate long names so the chip doesn't push the central nav
  // off-centre.
  const name = truncate(realName ?? seedName ?? (settled ? displayName(email) : null));

  useEffect(() => {
    // Persist only from a confirmed session (persist) and only real names, so the cache
    // never holds a heuristic or a name that disagrees with its email.
    if (persist && realName) writeIdentity({ email, name: realName });
  }, [persist, email, realName]);

  async function signOut() {
    // Shell-owned logout (site-root /api/auth): revokes the shared session and clears
    // its cookies for every zone at once.
    await fetch("/api/auth/logout", { method: "POST", headers: csrfHeader() });
    // The reads cached for this account go with the session. The hard navigation below
    // clears the in-memory half anyway; this is what clears the sessionStorage half.
    clearResources();
    window.location.href = withBasePath("/loggedout");
  }

  return (
    <div className={cn("flex items-center gap-2", className)}>
      <a
        href={withBasePath("/profile")}
        className={cn("flex min-w-0 items-center gap-2.5 rounded-lg px-1.5 py-1 transition-colors hover:bg-foreground/5", CHIP_FOCUS)}
      >
        <span className="flex size-8.5 shrink-0 items-center justify-center rounded-full bg-main-accent-t1/15 text-xs font-semibold text-main-accent-t1">
          {initialsOf(email)}
        </span>
        <div className="min-w-0">
          {name ? (
            <p className="truncate text-sm font-semibold text-foreground">{name}</p>
          ) : (
            <span className="my-1 block h-3 w-24 animate-pulse rounded bg-foreground/10" aria-hidden />
          )}
          <p className="flex items-center gap-1 text-xs font-medium text-main-accent-t1">
            <BadgeCheck className="size-3" /> Verified
          </p>
        </div>
      </a>
      <button
        type="button"
        onClick={signOut}
        aria-label="Sign out"
        className={cn("shrink-0 rounded-md text-muted-foreground transition-colors hover:text-foreground", CHIP_FOCUS)}
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
        "inline-flex h-9 items-center justify-center rounded-md border border-main-accent-t1 bg-transparent px-4 font-mono-tech text-xs tracking-wider text-main-accent-t1 transition-all duration-300 hover:bg-primary hover:text-primary-foreground",
        CHIP_FOCUS,
        className,
      )}
    >
      Investor Portal
    </a>
  );
}

function ChipSkeleton({ className }: { className?: string }) {
  return (
    <div className={cn("flex items-center gap-2.5 px-1.5 py-1", className)} aria-hidden>
      <span className="size-8.5 shrink-0 animate-pulse rounded-full bg-foreground/10" />
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

/** Keep the chip label from pushing the central nav off-centre. */
function truncate(s: string | null, max = 28): string | null {
  if (!s) return s;
  if (s.length <= max) return s;
  return `${s.slice(0, max - 1)}…`;
}
