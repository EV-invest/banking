"use client";

import { BadgeCheck, Loader2 } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";

import { Avatar, AvatarFallback, Button, Input, Skeleton } from "@evinvest/uikit";

import { fetchPositions } from "@/entities/fund/api/fund-client";
import { fetchProfile, saveProfile } from "@/entities/user/api/profile-client";
import type { Position, UpdateProfileRequest, UserProfile } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { displayName } from "@/views/profile/lib/format";

const CARD = "rounded-[14px] border border-border bg-main-card";

// All editable fields are held in the form (initialised from the loaded profile) even
// though this surface only shows seven of them — UpdateProfile is full-replace, so
// carrying language/currency/timezone preserves what Settings → General owns.
const EDITABLE = ["legal_name", "preferred_name", "phone", "date_of_birth", "nationality", "tax_residence", "residential_address", "language", "base_currency", "timezone"] as const;
type Form = Record<(typeof EDITABLE)[number], string>;

const SHOWN: { key: (typeof EDITABLE)[number]; label: string }[] = [
  { key: "legal_name", label: "Legal name" },
  { key: "preferred_name", label: "Preferred name" },
  { key: "phone", label: "Phone number" },
  { key: "date_of_birth", label: "Date of birth" },
  { key: "nationality", label: "Nationality" },
  { key: "tax_residence", label: "Tax residence" },
  { key: "residential_address", label: "Residential address" },
];

function formFrom(p: UserProfile): Form {
  return Object.fromEntries(EDITABLE.map((k) => [k, p[k] ?? ""])) as Form;
}

// The investor profile (Figma `cabinet/profile`). Name/email + the editable personal
// fields are real (core UsersService via the BFF); the Account snapshot is real (fund
// positions). There is no KYC system, so that section is intentionally omitted.
export function ProfileView() {
  const [profile, setProfile] = useState<UserProfile | null | undefined>(undefined);
  const [positions, setPositions] = useState<Position[]>([]);
  const [form, setForm] = useState<Form | null>(null);
  const [editing, setEditing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchProfile()
      .then((p) => {
        setProfile(p);
        setForm(formFrom(p));
      })
      .catch((e: Error) => {
        setProfile(null);
        setError(e.message);
      });
    fetchPositions()
      .then((l) => setPositions(l.positions ?? []))
      .catch(() => undefined);
  }, []);

  const loading = profile === undefined;
  const email = profile?.email ?? "";
  const legalName = (profile?.legal_name ?? "").trim();
  const name = legalName || (loading ? "…" : displayName(email));
  const invested = positions.reduce((s, p) => s + num(p.value), 0);

  function set(key: keyof Form, value: string) {
    setForm((f) => (f ? { ...f, [key]: value } : f));
  }
  function cancel() {
    if (profile) setForm(formFrom(profile));
    setEditing(false);
    setError(null);
  }
  async function save() {
    if (!form) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await saveProfile(form as UpdateProfileRequest);
      setProfile(updated);
      setForm(formFrom(updated));
      setEditing(false);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="flex flex-col gap-5 px-8 pb-8 pt-6">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <h1 className="font-sans text-2xl font-semibold text-foreground">Profile</h1>
          <p className="text-[13px] text-muted-foreground">Your personal details and verification status</p>
        </div>
        <div className="flex shrink-0 gap-2">
          {editing ? (
            <>
              <Button variant="outline" size="sm" className="border-border" onClick={cancel} disabled={saving}>
                Cancel
              </Button>
              <button
                type="button"
                onClick={save}
                disabled={saving}
                className="inline-flex items-center gap-1.5 rounded-lg bg-main-accent-t1 px-4 py-[9px] text-[13px] font-semibold text-main-black transition-opacity hover:opacity-90 disabled:opacity-60"
              >
                {saving && <Loader2 className="size-4 animate-spin" />} Save
              </button>
            </>
          ) : (
            <button
              type="button"
              onClick={() => setEditing(true)}
              disabled={loading || profile === null}
              className="rounded-lg bg-main-accent-t1 px-4 py-[9px] text-[13px] font-semibold text-main-black transition-opacity hover:opacity-90 disabled:opacity-60"
            >
              Edit profile
            </button>
          )}
        </div>
      </div>

      {error && <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>}

      <div className={cn(CARD, "flex items-center gap-5 px-6 py-[22px]")}>
        <Avatar className="size-16 shrink-0 bg-main-accent-t1">
          <AvatarFallback className="bg-main-accent-t1 text-2xl font-semibold text-main-black">{loading ? "…" : avatarInitials(name)}</AvatarFallback>
        </Avatar>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
            {loading ? <Skeleton className="h-7 w-44" /> : <p className="text-[22px] font-semibold text-white">{name || "Account"}</p>}
            {loading ? <Skeleton className="h-4 w-52" /> : <p className="truncate text-[13px] text-muted-foreground">{email || "Not signed in"}</p>}
          </div>
          {!loading && profile?.email_verified && (
            <span className="mt-2.5 inline-flex items-center gap-1 rounded-full bg-main-accent-t1/15 px-[9px] py-1 text-[11px] font-semibold text-main-accent-t1">
              <BadgeCheck className="size-3" /> Verified
            </span>
          )}
        </div>
      </div>

      <div className="flex flex-col items-start gap-5 xl:flex-row">
        <section className={cn(CARD, "w-full flex-1 space-y-[18px] px-6 py-[22px]")}>
          <header>
            <h2 className="text-[15px] font-semibold text-white">Personal information</h2>
            <p className="text-xs text-muted-foreground">Used for compliance and statements</p>
          </header>
          <div className="flex flex-wrap gap-[16px_18px]">
            {SHOWN.map(({ key, label }) => (
              <FieldBox key={key} label={label}>
                {loading || !form ? (
                  <Skeleton className="h-[42px] w-full rounded-lg" />
                ) : editing ? (
                  <Input value={form[key]} onChange={(e) => set(key, e.target.value)} className="border-border bg-main-surface" />
                ) : (
                  <ReadValue value={profile?.[key]} />
                )}
              </FieldBox>
            ))}
            <FieldBox label="Email address" trailing={profile?.email_verified ? <VerifiedTag /> : undefined}>
              {loading ? <Skeleton className="h-[42px] w-full rounded-lg" /> : <ReadValue value={email} muted />}
            </FieldBox>
          </div>
        </section>

        <div className="flex w-full flex-col gap-5 xl:w-[388px]">
          <section className={cn(CARD, "px-[22px] pb-2 pt-5")}>
            <header className="pb-1">
              <h2 className="text-[15px] font-semibold text-white">Account snapshot</h2>
            </header>
            <SnapRow label="Total invested" first>
              <span className="text-[13px] font-semibold tabular-nums text-white">{money(invested)}</span>
            </SnapRow>
            <SnapRow label="Active strategies">
              <span className="text-[13px] font-semibold tabular-nums text-white">{positions.length}</span>
            </SnapRow>
          </section>
        </div>
      </div>
    </div>
  );
}

function FieldBox({ label, trailing, children }: { label: string; trailing?: ReactNode; children: ReactNode }) {
  return (
    <div className="min-w-[260px] flex-1">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-xs text-muted-foreground">{label}</span>
        {trailing}
      </div>
      {children}
    </div>
  );
}

function ReadValue({ value, muted }: { value?: string; muted?: boolean }) {
  const v = (value ?? "").trim();
  return <div className={cn("flex min-h-[42px] items-center rounded-lg border border-border bg-main-surface px-[13px] py-[11px] text-[13px]", v && !muted ? "text-white" : "text-muted-foreground")}>{v || "—"}</div>;
}

function VerifiedTag() {
  return (
    <span className="inline-flex items-center gap-1 text-[11px] font-semibold text-main-accent-t1">
      <BadgeCheck className="size-3" /> Verified
    </span>
  );
}

function SnapRow({ label, first, children }: { label: string; first?: boolean; children: ReactNode }) {
  return (
    <>
      {!first && <div className="h-px bg-border" />}
      <div className="flex items-center justify-between gap-4 py-[13px]">
        <p className="text-[13px] text-muted-foreground">{label}</p>
        <div className="shrink-0">{children}</div>
      </div>
    </>
  );
}

function avatarInitials(name: string): string {
  const parts = name.replace(/@.*/, "").split(/[\s._-]+/).filter(Boolean);
  return ((parts[0]?.[0] ?? "E") + (parts[1]?.[0] ?? "")).toUpperCase();
}

function num(value: string | undefined): number {
  const n = Number(value ?? "0");
  return Number.isFinite(n) ? n : 0;
}

function money(n: number): string {
  return n.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });
}
