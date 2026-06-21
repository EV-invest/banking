"use client";

import { BadgeCheck, Camera, ChevronDown } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";

import { Avatar, AvatarFallback, Skeleton } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { displayName, firstName, initialsOf } from "@/views/profile/lib/format";

const CARD = "rounded-[14px] border border-border bg-main-card";

// The investor profile surface (Figma `cabinet/profile`). Only the name + email are
// real — derived from the BFF session, like the sidebar AccountChip. The rest is an
// honest, styled scaffold: there is no profile/KYC API yet, so these read as the
// design intends rather than as fabricated live figures.
export function ProfileView() {
  const [email, setEmail] = useState<string | null | undefined>(undefined);

  useEffect(() => {
    let active = true;
    fetch("/api/auth/session")
      .then((r) => r.json() as Promise<{ authenticated: boolean; user?: { email: string } }>)
      .then((s) => {
        if (!active) return;
        setEmail(s.authenticated && s.user ? s.user.email : null);
      })
      .catch(() => {
        if (active) setEmail(null);
      });
    return () => {
      active = false;
    };
  }, []);

  const loading = email === undefined;
  const name = displayName(email);
  const first = firstName(email);

  return (
    <div className="flex flex-col gap-5 px-8 pb-8 pt-6">
      {/* topbar */}
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <h1 className="font-sans text-2xl font-semibold text-foreground">Profile</h1>
          <p className="text-[13px] text-muted-foreground">Your personal details and verification status</p>
        </div>
        <button type="button" className="shrink-0 rounded-lg bg-main-accent-t1 px-4 py-[9px] text-[13px] font-semibold text-main-black transition-opacity hover:opacity-90">
          Edit profile
        </button>
      </div>

      {/* header card */}
      <div className={cn(CARD, "flex items-center gap-5 px-6 py-[22px]")}>
        <Avatar className="size-16 shrink-0 bg-main-accent-t1">
          <AvatarFallback className="bg-main-accent-t1 text-2xl font-semibold text-main-black">{loading ? "…" : initialsOf(email)}</AvatarFallback>
        </Avatar>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
            {loading ? <Skeleton className="h-7 w-44" /> : <p className="text-[22px] font-semibold text-white">{name}</p>}
            {loading ? <Skeleton className="h-4 w-52" /> : <p className="truncate text-[13px] text-muted-foreground">{email ?? "Not signed in"}</p>}
          </div>
          <div className="mt-2.5 flex flex-wrap items-center gap-2">
            <Pill tone="teal">
              <BadgeCheck className="size-3" /> Verified
            </Pill>
            <Pill tone="amber">Accredited investor</Pill>
            <Pill tone="muted">Member since Jan 2025</Pill>
          </div>
        </div>
        <button type="button" className="flex h-8 shrink-0 items-center gap-1.5 rounded-md border border-border px-3 text-sm text-main-mist/90 transition-colors hover:bg-foreground/[0.04]">
          <Camera className="size-4" /> Change photo
        </button>
      </div>

      {/* content row */}
      <div className="flex flex-col items-start gap-5 xl:flex-row">
        {/* personal information */}
        <section className={cn(CARD, "w-full flex-1 space-y-[18px] px-6 py-[22px]")}>
          <header>
            <h2 className="text-[15px] font-semibold text-white">Personal information</h2>
            <p className="text-xs text-muted-foreground">Used for compliance and statements</p>
          </header>
          <div className="flex flex-wrap gap-[16px_18px]">
            <DisplayField label="Legal name" value={loading ? null : name} />
            <DisplayField label="Preferred name" value={loading ? null : first || name} />
            <DisplayField label="Email address" value={loading ? null : (email ?? "—")} verified />
            <DisplayField label="Phone number" value="+84 90 123 4567" />
            <DisplayField label="Date of birth" value="12 Mar 1990" />
            <DisplayField label="Nationality" value="Vietnam" select />
            <DisplayField label="Tax residence" value="Vietnam" select />
            <DisplayField label="Residential address" value="12 Tran Phu, Quy Nhon" />
          </div>
        </section>

        {/* right column */}
        <div className="flex w-full flex-col gap-5 xl:w-[388px]">
          <section className={cn(CARD, "px-[22px] pb-2 pt-5")}>
            <header className="pb-1">
              <h2 className="text-[15px] font-semibold text-white">Identity verification</h2>
              <p className="text-xs text-muted-foreground">KYC status and documents</p>
            </header>
            <StatusRow label="Government ID" sub="Passport ending 4821" first>
              <Pill tone="teal">
                <BadgeCheck className="size-3" /> Verified
              </Pill>
            </StatusRow>
            <StatusRow label="Proof of address" sub="Utility bill · Mar 2026">
              <Pill tone="teal">
                <BadgeCheck className="size-3" /> Verified
              </Pill>
            </StatusRow>
            <StatusRow label="Accredited investor" sub="Income & assets confirmed">
              <Pill tone="teal">
                <BadgeCheck className="size-3" /> Verified
              </Pill>
            </StatusRow>
            <StatusRow label="Source of funds" sub="Documents submitted">
              <Pill tone="amber">In review</Pill>
            </StatusRow>
          </section>

          <section className={cn(CARD, "px-[22px] pb-2 pt-5")}>
            <header className="pb-1">
              <h2 className="text-[15px] font-semibold text-white">Account snapshot</h2>
            </header>
            <StatusRow label="Total invested" first>
              <span className="text-[13px] font-semibold tabular-nums text-white">$40,750</span>
            </StatusRow>
            <StatusRow label="Active strategies">
              <span className="text-[13px] font-semibold tabular-nums text-white">3</span>
            </StatusRow>
            <StatusRow label="Auto-deploy idle cash">
              <span className="text-[13px] font-semibold text-main-accent-t1">On</span>
            </StatusRow>
            <StatusRow label="Statements available">
              <span className="text-[13px] font-semibold tabular-nums text-white">12</span>
            </StatusRow>
          </section>
        </div>
      </div>
    </div>
  );
}

function Pill({ tone, children }: { tone: "teal" | "amber" | "muted"; children: ReactNode }) {
  const toneClass =
    tone === "teal"
      ? "bg-main-accent-t1/15 text-main-accent-t1"
      : tone === "amber"
        ? "bg-main-accent-t3/15 text-main-accent-t3"
        : "bg-foreground/5 text-muted-foreground";
  return <span className={cn("inline-flex items-center gap-1 rounded-full px-[9px] py-1 text-[11px] font-semibold", toneClass)}>{children}</span>;
}

function DisplayField({ label, value, verified, select }: { label: string; value: string | null; verified?: boolean; select?: boolean }) {
  return (
    <div className="min-w-[260px] flex-1">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-xs text-muted-foreground">{label}</span>
        {verified && (
          <span className="inline-flex items-center gap-1 text-[11px] font-semibold text-main-accent-t1">
            <BadgeCheck className="size-3" /> Verified
          </span>
        )}
      </div>
      <div className="flex items-center gap-2 rounded-lg border border-border bg-main-surface px-[13px] py-[11px]">
        {value === null ? <Skeleton className="h-[18px] w-32" /> : <span className="min-w-0 flex-1 truncate text-[13px] text-white">{value}</span>}
        {select && <ChevronDown className="size-4 shrink-0 text-muted-foreground" />}
      </div>
    </div>
  );
}

function StatusRow({ label, sub, first, children }: { label: string; sub?: string; first?: boolean; children: ReactNode }) {
  return (
    <>
      {!first && <div className="h-px bg-border" />}
      <div className="flex items-center justify-between gap-4 py-[13px]">
        <div className="min-w-0">
          <p className="text-[13px] text-white">{label}</p>
          {sub && <p className="text-[11px] text-muted-foreground">{sub}</p>}
        </div>
        <div className="shrink-0">{children}</div>
      </div>
    </>
  );
}
