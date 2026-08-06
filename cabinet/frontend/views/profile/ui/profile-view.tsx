"use client";

import { BadgeCheck, Loader2 } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";

import { PhoneNumber } from "@evinvest/types";
import { usePhoneNumber } from "@evinvest/types/react";
import { Button, Input, Skeleton } from "@evinvest/uikit";

import { fetchPositions } from "@/entities/fund/api/fund-client";
import { fetchProfile, saveProfile } from "@/entities/user/api/profile-client";
import { publishProfile } from "@/entities/user/model/profile-store";
import { validateProfileForm } from "@/entities/user/model/profile-schema";
import type { Position, UpdateProfileRequest, UserProfile } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { formatUsd, num } from "@/shared/lib/money";
import { CARD, Hairline, InitialsAvatar, ListCard, ListCardTitle, Pill, type PillTone, Row, RowLabel, RowValue, StackRow } from "@/shared/ui/list-card";
import { MobileAppBar } from "@/shared/ui/mobile-appbar";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { displayName, initialsOfName, truncateName } from "@/views/profile/lib/format";

// The investor profile. Two Figma frames, one component: `cabinet/mobile/profile`
// (node 503:266) below `lg` — a hero card over stacked label/value cards — and
// `cabinet/profile` (node 489:258) above it, a two-column form beside the verification
// and snapshot cards.
//
// Name/email and the editable personal fields are real (core UsersService via the BFF),
// as are Identity verification (the admin-managed `status`/`kyc_level`/`email_verified`
// on the user record) and the Account snapshot (fund positions). The mock's per-document
// KYC rows (passport, proof of address, source of funds) have no document store behind
// them, so this states the KYC level the hub actually holds instead of inventing four.

// All editable fields are held in the form (initialised from the loaded profile) even
// though this surface only shows seven of them — UpdateProfile is full-replace, so
// carrying language/currency/timezone preserves what Settings → General owns.
const EDITABLE = ["legal_name", "preferred_name", "phone", "date_of_birth", "nationality", "tax_residence", "residential_address", "language", "base_currency", "timezone"] as const;
type Form = Record<(typeof EDITABLE)[number], string>;

const SHOWN: { key: (typeof EDITABLE)[number]; label: string; tip?: TipKey }[] = [
  { key: "legal_name", label: "Legal name", tip: "profile.field.legal-name" },
  { key: "preferred_name", label: "Preferred name" },
  { key: "phone", label: "Phone number" },
  { key: "date_of_birth", label: "Date of birth" },
  { key: "nationality", label: "Nationality", tip: "profile.field.nationality" },
  { key: "tax_residence", label: "Tax residence", tip: "profile.field.tax-residence" },
  { key: "residential_address", label: "Residential address" },
];

function formFrom(p: UserProfile): Form {
  return Object.fromEntries(EDITABLE.map((k) => [k, p[k] ?? ""])) as Form;
}

export function ProfileView() {
  const [profile, setProfile] = useState<UserProfile | null | undefined>(undefined);
  const [positions, setPositions] = useState<Position[]>([]);
  const [form, setForm] = useState<Form | null>(null);
  const [editing, setEditing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

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
  const name = truncateName(legalName) || (loading ? "…" : displayName(email));
  const invested = positions.reduce((s, p) => s + num(p.value), 0);

  function set(key: keyof Form, value: string) {
    setForm((f) => (f ? { ...f, [key]: value } : f));
  }
  function cancel() {
    if (profile) setForm(formFrom(profile));
    setEditing(false);
    setError(null);
    setFieldErrors({});
  }
  async function save() {
    if (!form) return;
    const errors = validateProfileForm(form);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      setError("Please fix the highlighted fields");
      return;
    }
    setFieldErrors({});
    setSaving(true);
    setError(null);
    try {
      const updated = await saveProfile(form as UpdateProfileRequest);
      setProfile(updated);
      setForm(formFrom(updated));
      publishProfile(updated); // the sidebar account chip picks up the new name live
      setEditing(false);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  }

  const verification = <VerificationCard loading={loading} profile={profile ?? null} email={email} />;
  const snapshot = <SnapshotCard invested={invested} strategies={positions.length} />;

  return (
    <>
      <MobileAppBar title="Profile" backHref="/settings" />

      <div className="flex flex-col gap-4 px-5 pb-6 pt-4.5 lg:gap-5 lg:px-8 lg:pb-8 lg:pt-6">
        {/* Desktop page heading — the mobile app bar owns this below `lg`. */}
        <div className="hidden items-center justify-between gap-4 lg:flex">
          <div className="min-w-0">
            <h1 className="font-sans text-2xl font-semibold text-foreground">Profile</h1>
            <p className="text-sm text-muted-foreground">Your personal details and verification status</p>
          </div>
          <div className="flex shrink-0 gap-2">
            {editing ? (
              <>
                <Button variant="outline" size="sm" className="border-border" onClick={cancel} disabled={saving}>
                  Cancel
                </Button>
                <Button type="button" onClick={save} disabled={saving} className="rounded-lg font-semibold">
                  {saving && <Loader2 className="size-4 animate-spin" />} Save
                </Button>
              </>
            ) : (
              <Button type="button" onClick={() => setEditing(true)} disabled={loading || profile === null} className="rounded-lg font-semibold">
                Edit profile
              </Button>
            )}
          </div>
        </div>

        {error && <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>}

        {/* card-Hero — centred on mobile (Figma 503:274), a wide chip on desktop (489:258). */}
        <div className={cn(CARD, "flex flex-col items-center gap-3 px-5 pb-5.5 pt-6 lg:flex-row lg:gap-5 lg:px-6 lg:py-5.5")}>
          <InitialsAvatar initials={initialsOfName(name, email)} className="size-16 text-2xl lg:text-xl" />
          <div className="flex min-w-0 flex-1 flex-col items-center gap-1 text-center lg:items-start lg:text-left">
            <div className="flex min-w-0 flex-col items-center gap-1 lg:flex-row lg:items-baseline lg:gap-3">
              {loading ? <Skeleton className="h-6 w-40" /> : <p className="truncate text-lg font-semibold text-foreground lg:text-xl">{name || "Account"}</p>}
              {loading ? <Skeleton className="h-4 w-48" /> : <p className="truncate text-sm text-muted-foreground">{email || "Not signed in"}</p>}
            </div>
            {!loading && (
              <div className="mt-1 flex flex-wrap items-center justify-center gap-2 lg:justify-start">
                {profile?.email_verified && <Pill icon={BadgeCheck}>Verified</Pill>}
                {profile?.status && <Pill tone={statusTone(profile.status)}>{titleCase(profile.status)}</Pill>}
                {profile?.kyc_level !== undefined && <Pill tone="neutral">KYC level {profile.kyc_level}</Pill>}
              </div>
            )}
          </div>
          {/* Mobile puts the primary action in the hero; desktop has it in the page heading. */}
          <div className="flex w-full gap-2 lg:hidden">
            {editing ? (
              <>
                <Button variant="outline" className="h-9 flex-1 border-input" onClick={cancel} disabled={saving}>
                  Cancel
                </Button>
                <Button type="button" onClick={save} disabled={saving} className="h-9 flex-1 gap-1.5 font-semibold">
                  {saving && <Loader2 className="size-4 animate-spin" />} Save
                </Button>
              </>
            ) : (
              <Button variant="outline" className="h-9 w-full border-input" onClick={() => setEditing(true)} disabled={loading || profile === null}>
                Edit profile
              </Button>
            )}
          </div>
        </div>

        {/* ── Mobile (Figma cabinet/mobile/profile) ────────────────────────── */}
        <div className="flex flex-col gap-4 lg:hidden">
          <ListCard>
            <ListCardTitle sub={profile?.role ? <span className="text-main-accent-t1/85">{titleCase(profile.role)} account</span> : undefined}>Personal information</ListCardTitle>
            {SHOWN.map(({ key, label }, i) => (
              <div key={key}>
                {i > 0 && <Hairline />}
                <StackRow label={label}>
                  {loading || !form ? (
                    <Skeleton className="h-5 w-40" />
                  ) : editing ? (
                    key === "phone" ? (
                      <PhoneField initial={form.phone} onChange={(v) => set("phone", v)} error={fieldErrors.phone} />
                    ) : (
                      <div className="min-w-0">
                        <Input
                          value={form[key]}
                          onChange={(e) => set(key, e.target.value)}
                          className={fieldErrors[key] ? "border-destructive bg-destructive/5" : "border-border bg-main-surface"}
                        />
                        {fieldErrors[key] && <p className="mt-1 text-xs text-destructive">{fieldErrors[key]}</p>}
                      </div>
                    )
                  ) : (
                    <span className={cn("break-words text-sm font-medium", profile?.[key] ? "text-foreground" : "text-muted-foreground")}>
                      {(key === "phone" ? formatPhone(profile?.[key]) : profile?.[key]) || "—"}
                    </span>
                  )}
                </StackRow>
              </div>
            ))}
            <Hairline />
            <StackRow label="Email address">
              {loading ? <Skeleton className="h-5 w-48" /> : <span className="break-words text-sm font-medium text-muted-foreground">{email || "—"}</span>}
            </StackRow>
          </ListCard>

          {verification}
          {snapshot}
        </div>

        {/* ── Desktop (Figma cabinet/profile) ──────────────────────────────── */}
        <div className="hidden items-start gap-5 lg:flex">
          <section className={cn(CARD, "w-full flex-1 space-y-4.5 px-6 py-5.5")}>
            <header>
              <h2 className="font-sans text-sm font-semibold tracking-normal text-foreground">Personal information</h2>
              <p className="text-xs text-muted-foreground">Used for compliance and statements</p>
            </header>
            <div className="flex flex-wrap gap-x-4.5 gap-y-4">
              {SHOWN.map(({ key, label, tip }) => (
                <FieldBox key={key} label={label} tip={tip}>
                  {loading || !form ? (
                    <Skeleton className="h-10.5 w-full rounded-lg" />
                  ) : editing ? (
                    key === "phone" ? (
                      <PhoneField initial={form.phone} onChange={(v) => set("phone", v)} error={fieldErrors.phone} />
                    ) : (
                      <div className="min-w-0 flex-1">
                        <Input
                          value={form[key]}
                          onChange={(e) => set(key, e.target.value)}
                          className={fieldErrors[key] ? "border-destructive bg-destructive/5" : "border-border bg-main-surface"}
                        />
                        {fieldErrors[key] && <p className="mt-1 text-xs text-destructive">{fieldErrors[key]}</p>}
                      </div>
                    )
                  ) : (
                    <ReadValue value={key === "phone" ? formatPhone(profile?.[key]) : profile?.[key]} />
                  )}
                </FieldBox>
              ))}
              <FieldBox label="Email address" trailing={profile?.email_verified ? <VerifiedTag /> : undefined}>
                {loading ? <Skeleton className="h-10.5 w-full rounded-lg" /> : <ReadValue value={email} muted />}
              </FieldBox>
            </div>
          </section>

          <div className="flex w-97 shrink-0 flex-col gap-5">
            {verification}
            {snapshot}
          </div>
        </div>
      </div>
    </>
  );
}

/** `card-Verification` — the KYC state the hub actually holds, not a document checklist. */
function VerificationCard({ loading, profile, email }: { loading: boolean; profile: UserProfile | null; email: string }) {
  return (
    <ListCard className="lg:px-5.5">
      <ListCardTitle sub="Managed by compliance">Identity verification</ListCardTitle>
      <Hairline />
      <Row>
        <RowLabel title="Email address" sub={loading ? "…" : email || "—"} />
        {loading ? <Skeleton className="h-5 w-16 rounded-full" /> : profile?.email_verified ? <Pill icon={BadgeCheck}>Verified</Pill> : <Pill tone="pending">Unverified</Pill>}
      </Row>
      <Hairline />
      <Row>
        <RowLabel title="KYC level" sub="Raised by compliance review" />
        {loading ? <Skeleton className="h-4 w-10" /> : <RowValue className="font-semibold tabular-nums text-foreground">{profile?.kyc_level ?? "—"}</RowValue>}
      </Row>
      <Hairline />
      <Row>
        <RowLabel title="Account status" sub="Platform access" />
        {loading ? <Skeleton className="h-5 w-16 rounded-full" /> : profile?.status ? <Pill tone={statusTone(profile.status)}>{titleCase(profile.status)}</Pill> : <RowValue>—</RowValue>}
      </Row>
    </ListCard>
  );
}

function SnapshotCard({ invested, strategies }: { invested: number; strategies: number }) {
  return (
    <ListCard className="lg:px-5.5">
      <ListCardTitle>Account snapshot</ListCardTitle>
      <Hairline />
      <Row>
        <span className="text-sm font-medium text-muted-foreground">Total invested</span>
        <span className="text-sm font-semibold tabular-nums text-foreground">{formatUsd(invested)}</span>
      </Row>
      <Hairline />
      <Row>
        <span className="text-sm font-medium text-muted-foreground">Active strategies</span>
        <span className="text-sm font-semibold tabular-nums text-foreground">{strategies}</span>
      </Row>
    </ListCard>
  );
}

function FieldBox({ label, trailing, tip, children }: { label: string; trailing?: ReactNode; tip?: TipKey; children: ReactNode }) {
  return (
    <div className="flex min-w-65 flex-1 flex-col">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          {label}
          {tip && <TipAnchor anchor={tip} />}
        </span>
        {trailing}
      </div>
      {children}
    </div>
  );
}

const MAX_DISPLAY_VALUE = 80;

function ReadValue({ value, muted }: { value?: string; muted?: boolean }) {
  const raw = (value ?? "").trim();
  const v = raw.length > MAX_DISPLAY_VALUE ? `${raw.slice(0, MAX_DISPLAY_VALUE - 1)}…` : raw;
  return (
    <div
      className={cn(
        "flex min-h-10.5 items-center rounded-lg border border-border bg-main-surface px-3.5 py-3 text-sm",
        v && !muted ? "text-foreground" : "text-muted-foreground",
      )}
      title={raw.length > MAX_DISPLAY_VALUE ? raw : undefined}
    >
      {v || "—"}
    </div>
  );
}

function VerifiedTag() {
  return (
    <span className="inline-flex items-center gap-1 text-xs font-semibold text-main-accent-t1">
      <BadgeCheck className="size-3" /> Verified
      <TipAnchor anchor="profile.email.verified" />
    </span>
  );
}

/** `status`/`role` arrive as lower snake_case from the hub. */
function titleCase(value: string): string {
  const s = value.replace(/_/g, " ");
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function statusTone(status: string): PillTone {
  const s = status.toLowerCase();
  if (s === "active") return "positive";
  if (s === "pending" || s === "review") return "pending";
  return "neutral";
}

function formatPhone(raw?: string): string {
  if (!raw) return "";
  const pn = PhoneNumber.parseInput(raw) ?? PhoneNumber.fromUnsafe(raw);
  return PhoneNumber.format(pn);
}

/** Phone input wired to the `PhoneNumber` TypeObject via `usePhoneNumber`. */
function PhoneField({ initial, onChange, error }: { initial: string; onChange: (v: string) => void; error?: string }) {
  const { inputProps, value, reset } = usePhoneNumber({ initial: initial || undefined });
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  // Propagate canonical value changes to the parent form state.
  useEffect(() => {
    onChangeRef.current(value ? PhoneNumber.raw(value) : "");
  }, [value]);

  // Sync external initial changes (e.g. after save reloads the profile).
  useEffect(() => {
    if (initial) reset(initial);
  }, [initial, reset]);

  return (
    <div className="min-w-0 flex-1">
      <Input {...inputProps} className={error ? "border-destructive bg-destructive/5" : "border-border bg-main-surface"} />
      {error && <p className="mt-1 text-xs text-destructive">{error}</p>}
    </div>
  );
}
