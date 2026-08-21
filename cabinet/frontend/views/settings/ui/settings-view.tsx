"use client";

import { useT } from "@evinvest/i18n/react";

import { BadgeCheck, Bell, Check, Laptop, Loader2, LogOut, type LucideIcon, Monitor, Shield, Smartphone, User } from "lucide-react";
import { Link } from "@/shared/ui/cabinet-link";
import { type ReactNode, useEffect, useRef, useState } from "react";

import { Email, PhoneNumber } from "@evinvest/types";
import { usePhoneNumber } from "@evinvest/types/react";
import { Badge, Button, Input, Select, SelectContent, SelectItem, SelectTrigger, Skeleton, Switch } from "@evinvest/uikit";

import {
  notificationSettingsResource,
  setChannelEnabled,
  setTopicSubscription,
} from "@/entities/notification/model/notification-resource";
import { refreshUnreadCount } from "@/entities/notification/model/notification-store";
import { revokeSession, sessionsResource } from "@/entities/session/model/session-resource";
import { profileResource, saveProfile } from "@/entities/user/model/profile-resource";
import { validateProfileForm } from "@/entities/user/model/profile-schema";
import { withBasePath } from "@/shared/config/base-path";
import type { Session, UpdateProfileRequest, UserProfile } from "@/shared/contracts";
import type { NotificationSettings } from "@/shared/contracts/notifications";
import { cn } from "@/shared/lib/cn";
import { csrfHeader } from "@/shared/lib/csrf-client";
import { useResource } from "@/shared/lib/resource";
import { TipAnchor } from "@/shared/tips";
import { CARD, Chevron, Hairline, InitialsAvatar, ListCard, ListCardTitle, Pill, Row, RowLabel, RowValue } from "@/shared/ui/list-card";
import { MobileAppBar } from "@/shared/ui/mobile-appbar";
import { displayName, initialsOfName, truncateName } from "@/views/settings/lib/format";

// The investor settings surface, wired to the backend over the BFF. Two Figma frames,
// one component: `cabinet/mobile/settings` (node 498:259) below `lg` — an app bar over a
// stack of row cards, with Sessions pushed as its own screen — and `cabinet/settings`
// (node 481:250) above it, a section rail beside the editing form.
//
// General edits the same core user record as the Profile page (full-replace, so the form
// carries every editable field even where a breakpoint only shows some); Security states
// the real auth model (Google-managed) and surfaces live sessions; Sessions & devices
// lists and revokes the real refresh-token families at the hub; Notifications is the real
// delivery-preference store. Auth is Google-OAuth-only and there is no theme store, so the
// mock's 2FA/biometric/password rows and its Preferences card have no backing here — they
// are left out rather than faked.

const EDITABLE = ["legal_name", "preferred_name", "phone", "date_of_birth", "nationality", "tax_residence", "residential_address", "language", "base_currency", "timezone"] as const;
type Form = Record<(typeof EDITABLE)[number], string>;

function formFrom(p: UserProfile): Form {
  return Object.fromEntries(EDITABLE.map((k) => [k, p[k] ?? ""])) as Form;
}

type Section = "general" | "security" | "sessions" | "notifications";

const NAV: { id: Section; label: string; icon: LucideIcon }[] = [
  { id: "general", label: "General", icon: User },
  { id: "security", label: "Security", icon: Shield },
  { id: "sessions", label: "Sessions & devices", icon: Monitor },
  { id: "notifications", label: "Notifications", icon: Bell },
];

export function SettingsView() {
  const [section, setSection] = useState<Section>("general");
  // The mobile stack: the root screen, or a section pushed on top of it.
  const [pushed, setPushed] = useState<"sessions" | "notifications" | null>(null);

  const [form, setForm] = useState<Form | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const savedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [revokeError, setRevokeError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // The profile is the same cached read the account chip and the Profile page use; the
  // session list is its own, refreshed by the revoke below rather than by a manual reload.
  const { data: profile, error: profileError, isLoading: loading } = useResource(profileResource);
  const sessionList = useResource(sessionsResource);
  const sessions = sessionList.data;
  const error = saveError ?? (profile ? null : (profileError?.message ?? null));
  const sessionsError = revokeError ?? (sessions ? null : (sessionList.error?.message ?? null));

  // Seeded during render, not in an effect, so a cached profile fills the form on the frame
  // it is read. Held back while the form is dirty: a background refresh must not overwrite
  // edits in progress.
  const [seeded, setSeeded] = useState<UserProfile | null>(null);
  const pristine = !form || !seeded || EDITABLE.every((k) => form[k] === (seeded[k] ?? ""));
  if (profile && profile !== seeded && pristine) {
    setSeeded(profile);
    setForm(formFrom(profile));
  }

  const email = profile?.email ?? null;
  const name = truncateName((profile?.legal_name ?? "").trim()) || displayName(email);
  const dirty = !pristine;

  function set(key: keyof Form, value: string) {
    setSaved(false);
    setForm((f) => (f ? { ...f, [key]: value } : f));
  }
  async function save() {
    if (!form || saving) return;
    const errors = validateProfileForm(form);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      setSaveError("Please fix the highlighted fields");
      return;
    }
    setFieldErrors({});
    setSaving(true);
    setSaveError(null);
    try {
      // Published into the cache by `saveProfile`, so the sidebar account chip and the
      // Profile page pick up the new name without a refetch.
      const updated = await saveProfile(form as UpdateProfileRequest);
      setForm(formFrom(updated));
      setSaved(true);
      if (savedTimer.current) clearTimeout(savedTimer.current);
      savedTimer.current = setTimeout(() => setSaved(false), 2500);
    } catch (e) {
      setSaveError((e as Error).message);
    } finally {
      setSaving(false);
    }
  }
  useEffect(
    () => () => {
      if (savedTimer.current) clearTimeout(savedTimer.current);
    },
    [],
  );

  async function revoke(id: string) {
    setBusy(true);
    setRevokeError(null);
    try {
      // The revoke invalidates the session list, so it refreshes itself.
      await revokeSession(id);
    } catch (e) {
      setRevokeError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  async function revokeOthers() {
    const others = (sessions ?? []).filter((s) => !s.current && s.id);
    if (!others.length) return;
    setBusy(true);
    setRevokeError(null);
    try {
      for (const s of others) await revokeSession(s.id!);
    } catch (e) {
      setRevokeError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  const sessionsPanel = (titled: boolean) => (
    <SessionsSection titled={titled} sessions={sessions} error={sessionsError} busy={busy} name={name} onRevoke={revoke} onRevokeOthers={revokeOthers} />
  );

  return (
    <>
      <MobileAppBar
        title={pushed === "sessions" ? "Sessions & devices" : pushed === "notifications" ? "Notifications" : "Settings"}
        onBack={pushed ? () => setPushed(null) : undefined}
        right={
          dirty ? (
            <Button type="button" size="sm" onClick={save} disabled={saving} className="rounded-full font-semibold">
              {saving && <Loader2 className="size-3.5 animate-spin" />} Save
            </Button>
          ) : pushed ? undefined : (
            <InitialsAvatar initials={initialsOfName(name, email)} className="size-8.5 text-sm" />
          )
        }
      />

      <div className="flex flex-col gap-4 px-5 pb-6 pt-4.5 lg:gap-6 lg:px-8 lg:pb-10 lg:pt-6">
        {/* Desktop page heading — the mobile app bar owns this below `lg`. */}
        <div className="hidden items-center justify-between gap-4 lg:flex">
          <div className="min-w-0">
            <h1 className="text-2xl font-semibold text-foreground">Settings</h1>
            <p className="text-sm text-muted-foreground">Manage your account, security and access</p>
          </div>
          {section === "general" && (
            <div className="flex shrink-0 items-center gap-3">
              {saved && (
                <span className="inline-flex items-center gap-1 text-sm font-medium text-main-accent-t2">
                  <Check className="size-4" /> Saved
                </span>
              )}
              <Button type="button" onClick={save} disabled={loading || saving || !dirty} className="rounded-lg font-semibold">
                {saving && <Loader2 className="size-4 animate-spin" />} Save changes
              </Button>
            </div>
          )}
        </div>

        {error && <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>}
        {saved && (
          <p className="inline-flex items-center gap-1 text-sm font-medium text-main-accent-t2 lg:hidden">
            <Check className="size-4" /> Saved
          </p>
        )}

        {/* ── Mobile (Figma cabinet/mobile/settings) ───────────────────────── */}
        <div className="flex flex-col gap-4 lg:hidden">
          {pushed === "sessions" ? (
            sessionsPanel(false)
          ) : pushed === "notifications" ? (
            <NotificationsSection />
          ) : (
            <>
              <ProfileSummaryCard loading={loading} name={name} email={email} verified={!!profile?.email_verified} />
              <AccountRowsCard loading={loading} form={form} email={email} fieldErrors={fieldErrors} onChange={set} />
              <MobileSecurityCard loading={loading} email={email} sessions={sessions} onOpenSessions={() => setPushed("sessions")} />
              <MobileNotificationsCard onOpen={() => setPushed("notifications")} />
              <SignOutButton />
            </>
          )}
        </div>

        {/* ── Desktop (Figma cabinet/settings) ─────────────────────────────── */}
        <div className="hidden gap-6 lg:flex">
          {/* Hand-written rail — uikit has no section-nav component, so the items carry their own focus ring. */}
          <nav aria-label="Settings sections" className="flex w-53 shrink-0 flex-col gap-1">
            {NAV.map((item) => {
              const Icon = item.icon;
              const active = section === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  aria-current={active ? "page" : undefined}
                  onClick={() => setSection(item.id)}
                  className={cn(
                    "flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring",
                    active ? "bg-main-accent-t1/15 font-semibold text-main-accent-t1" : "text-foreground hover:bg-foreground/5",
                  )}
                >
                  <Icon className="size-4.5" />
                  {item.label}
                </button>
              );
            })}
          </nav>

          <div className="min-w-0 flex-1">
            {section === "general" && <GeneralSection loading={loading} form={form} email={email} verified={!!profile?.email_verified} onChange={set} fieldErrors={fieldErrors} />}
            {section === "security" && <SecuritySection email={email} loading={loading} sessions={sessions} onManageSessions={() => setSection("sessions")} />}
            {section === "notifications" && <NotificationsSection />}
            {section === "sessions" && sessionsPanel(true)}
          </div>
        </div>
      </div>
    </>
  );
}

const LANGUAGES = [
  { value: "en", label: "English" },
  { value: "vi", label: "Tiếng Việt" },
];
const CURRENCIES = [
  { value: "USD", label: "USD ($)" },
  { value: "EUR", label: "EUR (€)" },
];
const TIMEZONES = [
  { value: "Asia/Ho_Chi_Minh", label: "Asia / Ho Chi Minh" },
  { value: "UTC", label: "UTC" },
];

function labelOf(options: { value: string; label: string }[], value: string): string {
  return options.find((o) => o.value === value)?.label ?? value;
}

/* ── Mobile cards ──────────────────────────────────────────────────────── */

/** `card-ProfileSummary` — the tap target into the full profile. */
function ProfileSummaryCard({ loading, name, email, verified }: { loading: boolean; name: string; email: string | null; verified: boolean }) {
  return (
    <Link
      href="/profile"
      className={cn(CARD, "flex items-center gap-3.5 py-3.5 pl-3.5 pr-4 outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring active:bg-foreground/5")}
    >
      <InitialsAvatar initials={initialsOfName(name, email)} className="size-11.5 text-base" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        {loading ? (
          <Skeleton className="h-4 w-32" />
        ) : (
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate text-sm font-semibold text-foreground">{name}</span>
            {verified && <Pill icon={BadgeCheck}>Verified</Pill>}
          </div>
        )}
        {loading ? <Skeleton className="h-3 w-40" /> : <span className="truncate text-xs text-muted-foreground">{formatEmail(email) || "Not signed in"}</span>}
      </div>
      <Chevron className="size-5" />
    </Link>
  );
}

/** `card-Account` — one tappable row per editable preference; tapping expands the editor in place. */
function AccountRowsCard({
  loading,
  form,
  email,
  fieldErrors,
  onChange,
}: {
  loading: boolean;
  form: Form | null;
  email: string | null;
  fieldErrors: Record<string, string>;
  onChange: (key: keyof Form, value: string) => void;
}) {
  const [open, setOpen] = useState<keyof Form | null>(null);
  const ready = !loading && !!form;
  const toggle = (key: keyof Form) => setOpen((k) => (k === key ? null : key));

  return (
    <ListCard>
      {/* Email is the IdP's — displayed, never editable here. */}
      <Row>
        <span className="shrink-0 text-sm font-medium text-foreground">Email</span>
        {loading ? <Skeleton className="h-3.5 w-36" /> : <RowValue>{formatEmail(email) || "—"}</RowValue>}
      </Row>
      <Hairline />
      <ExpandableRow label="Phone" value={form ? formatPhone(form.phone) : ""} loading={!ready} open={open === "phone"} onToggle={() => toggle("phone")}>
        {form && <PhoneField initial={form.phone} onChange={(v) => onChange("phone", v)} error={fieldErrors.phone} />}
      </ExpandableRow>
      <Hairline />
      <ExpandableRow label="Language" value={form ? labelOf(LANGUAGES, form.language) : ""} loading={!ready} open={open === "language"} onToggle={() => toggle("language")}>
        {form && <ThemedSelect value={form.language} onChange={(v) => onChange("language", v)} options={LANGUAGES} placeholder="Select language" error={fieldErrors.language} />}
      </ExpandableRow>
      <Hairline />
      <ExpandableRow label="Base currency" value={form ? labelOf(CURRENCIES, form.base_currency) : ""} loading={!ready} open={open === "base_currency"} onToggle={() => toggle("base_currency")}>
        {form && <ThemedSelect value={form.base_currency} onChange={(v) => onChange("base_currency", v)} options={CURRENCIES} placeholder="Select currency" error={fieldErrors.base_currency} />}
      </ExpandableRow>
      <Hairline />
      <ExpandableRow label="Time zone" value={form ? labelOf(TIMEZONES, form.timezone) : ""} loading={!ready} open={open === "timezone"} onToggle={() => toggle("timezone")}>
        {form && <ThemedSelect value={form.timezone} onChange={(v) => onChange("timezone", v)} options={TIMEZONES} placeholder="Select time zone" error={fieldErrors.timezone} />}
      </ExpandableRow>
    </ListCard>
  );
}

function ExpandableRow({
  label,
  value,
  loading,
  open,
  onToggle,
  children,
}: {
  label: string;
  value: string;
  loading: boolean;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col">
      <button
        type="button"
        onClick={onToggle}
        disabled={loading}
        aria-expanded={open}
        className="flex min-w-0 items-center justify-between gap-3 rounded-md py-3.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span className="shrink-0 text-sm font-medium text-foreground">{label}</span>
        <span className="flex min-w-0 items-center gap-2">
          {loading ? <Skeleton className="h-3.5 w-28" /> : !open && <RowValue>{value || "—"}</RowValue>}
          <Chevron className={cn("transition-transform", open && "rotate-90")} />
        </span>
      </button>
      {open && <div className="pb-3.5">{children}</div>}
    </div>
  );
}

/** `card-Security`, restated for the real auth model: Google-managed sign-in + live sessions. */
function MobileSecurityCard({
  loading,
  email,
  sessions,
  onOpenSessions,
}: {
  loading: boolean;
  email: string | null;
  sessions: Session[] | undefined;
  onOpenSessions: () => void;
}) {
  const t = useT();
  return (
    <ListCard>
      <ListCardTitle>Security</ListCardTitle>
      <Hairline />
      <Row>
        <RowLabel title={t("ui.signedInGoogle")} sub={loading ? "…" : formatEmail(email) || "—"} />
        <Pill>Connected</Pill>
      </Row>
      <Hairline />
      <button
        type="button"
        onClick={onOpenSessions}
        className="flex min-w-0 items-center justify-between gap-3 rounded-md py-3.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <RowLabel title={t("ui.trustedSessions")} sub="Devices signed in to your account" />
        <span className="flex shrink-0 items-center gap-2">
          {sessions === undefined ? <Skeleton className="h-3.5 w-4" /> : <RowValue>{sessions.length}</RowValue>}
          <Chevron />
        </span>
      </button>
      <Hairline />
      <p className="py-3 text-xs leading-relaxed text-muted-foreground">
        Your password, two-factor authentication and recovery are configured in your Google Account.
      </p>
    </ListCard>
  );
}

/** `card-Notifications` — the entry into delivery preferences. The mock's four topic
 *  switches live on the pushed screen, where the real per-topic state comes from. */
function MobileNotificationsCard({ onOpen }: { onOpen: () => void }) {
  const t = useT();
  return (
    <ListCard>
      <ListCardTitle>Notifications</ListCardTitle>
      <Hairline />
      <button
        type="button"
        onClick={onOpen}
        className="flex min-w-0 items-center justify-between gap-3 rounded-md py-3.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <RowLabel title={t("ui.deliveryTopics")} sub="Where we reach you and what you follow" />
        <Chevron />
      </button>
    </ListCard>
  );
}

/** `btn-SignOut` — the shell-owned logout, mirroring the header account chip. */
function SignOutButton() {
  const [busy, setBusy] = useState(false);
  async function signOut() {
    setBusy(true);
    // Site-root /api/auth: revokes the shared session and clears its cookies for every zone.
    await fetch("/api/auth/logout", { method: "POST", headers: csrfHeader() });
    window.location.href = withBasePath("/loggedout");
  }
  // Kept hand-written: at 48px tall it sits between uikit's `sm` (32) and `lg` (40)
  // Button heights, so it carries its own focus ring rather than being resized.
  return (
    <button
      type="button"
      onClick={signOut}
      disabled={busy}
      className="flex w-full items-center justify-center gap-2 rounded-lg border border-border py-3.5 text-sm font-semibold text-destructive outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring active:bg-destructive/10 disabled:opacity-60"
    >
      {busy ? <Loader2 className="size-4 animate-spin" /> : <LogOut className="size-4" />} Sign out
    </button>
  );
}

/* ── Desktop sections ──────────────────────────────────────────────────── */

function GeneralSection({
  loading,
  form,
  email,
  verified,
  onChange,
  fieldErrors,
}: {
  loading: boolean;
  form: Form | null;
  email: string | null;
  verified: boolean;
  onChange: (key: keyof Form, value: string) => void;
  fieldErrors: Record<string, string>;
}) {
  const t = useT();
  const ready = !loading && !!form;
  return (
    <section className={cn(CARD, "px-6 py-5.5")}>
      <SectionHeader title={t("ui.account")} sub="Your contact details and preferences" />
      <div className="flex flex-wrap gap-x-4.5 gap-y-4">
        <Field label="Legal name">
          {ready ? (
            <div className="min-w-0 flex-1">
              <Input value={form.legal_name} onChange={(e) => onChange("legal_name", e.target.value)} className={fieldErrors.legal_name ? "border-destructive bg-destructive/5" : "border-border bg-main-surface"} />
              {fieldErrors.legal_name && <p className="mt-1 text-xs text-destructive">{fieldErrors.legal_name}</p>}
            </div>
          ) : <FieldSkeleton />}
        </Field>
        <Field label="Preferred name">
          {ready ? (
            <div className="min-w-0 flex-1">
              <Input value={form.preferred_name} onChange={(e) => onChange("preferred_name", e.target.value)} className={fieldErrors.preferred_name ? "border-destructive bg-destructive/5" : "border-border bg-main-surface"} />
              {fieldErrors.preferred_name && <p className="mt-1 text-xs text-destructive">{fieldErrors.preferred_name}</p>}
            </div>
          ) : <FieldSkeleton />}
        </Field>
        <Field label="Email address" trailing={verified ? <VerifiedTag /> : undefined}>
          {loading ? <FieldSkeleton /> : <Input value={formatEmail(email)} readOnly className="border-border bg-main-surface text-muted-foreground" />}
        </Field>
        <Field label="Phone number">
          {ready ? <PhoneField initial={form.phone} onChange={(v) => onChange("phone", v)} error={fieldErrors.phone} /> : <FieldSkeleton />}
        </Field>
        <Field label="Language">
          {ready ? <ThemedSelect value={form.language} onChange={(v) => onChange("language", v)} options={LANGUAGES} placeholder="Select language" error={fieldErrors.language} /> : <FieldSkeleton />}
        </Field>
        <Field label="Base currency">
          {ready ? <ThemedSelect value={form.base_currency} onChange={(v) => onChange("base_currency", v)} options={CURRENCIES} placeholder="Select currency" error={fieldErrors.base_currency} /> : <FieldSkeleton />}
        </Field>
        <Field label="Time zone">
          {ready ? <ThemedSelect value={form.timezone} onChange={(v) => onChange("timezone", v)} options={TIMEZONES} placeholder="Select time zone" error={fieldErrors.timezone} /> : <FieldSkeleton />}
        </Field>
      </div>
    </section>
  );
}

function SecuritySection({
  email,
  loading,
  sessions,
  onManageSessions,
}: {
  email: string | null;
  loading: boolean;
  sessions: Session[] | undefined;
  onManageSessions: () => void;
}) {
  const t = useT();
  const count = sessions?.length;
  const summary = count === undefined ? "Loading active sessions…" : count === 1 ? "1 device currently signed in" : `${count} devices currently signed in`;
  return (
    <section className={cn(CARD, "px-6 py-5.5")}>
      <SectionHeader title={t("ui.security")} sub="How you sign in and where your account is active" />
      <div className="flex items-center gap-3 rounded-xl border border-border bg-main-surface px-4 py-3.5">
        {/* Google's mark is only licensed on a white plate, so this one square stays off-theme. */}
        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-white">
          <GoogleMark />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium text-foreground">Signed in with Google</p>
          {loading ? <Skeleton className="mt-1 h-3.5 w-44" /> : <p className="truncate text-xs text-muted-foreground">{formatEmail(email) || "—"}</p>}
        </div>
        <Badge className="border-transparent bg-main-accent-t1/15 text-main-accent-t1">Connected</Badge>
      </div>
      <p className="mb-1 mt-3 text-sm leading-relaxed text-muted-foreground">
        Your sign-in and password are managed by Google. Two-factor authentication and recovery are configured in your Google Account.
      </p>
      <Hairline />
      <Row>
        <RowLabel title={t("ui.sessionsDevices")} sub={summary} />
        <Button variant="outline" size="sm" className="border-border" onClick={onManageSessions}>
          Manage
        </Button>
      </Row>
    </section>
  );
}

function SessionsSection({
  titled,
  sessions,
  error,
  busy,
  name,
  onRevoke,
  onRevokeOthers,
}: {
  titled: boolean;
  sessions: Session[] | undefined;
  error: string | null;
  busy: boolean;
  name: string;
  onRevoke: (id: string) => void;
  onRevokeOthers: () => void;
}) {
  const loading = sessions === undefined;
  const list = sessions ?? [];
  const hasOthers = list.some((s) => !s.current);
  return (
    <ListCard className="lg:px-6 lg:pb-5.5 lg:pt-2">
      {titled ? (
        <ListCardTitle sub="Where you're signed in — revoke anything you don't recognise">Sessions &amp; devices</ListCardTitle>
      ) : (
        <p className="pb-2 pt-3 text-xs font-medium text-muted-foreground">Where you&apos;re signed in — revoke anything you don&apos;t recognise</p>
      )}

      {error && <p className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>}

      {loading ? (
        [0, 1].map((i) => (
          <div key={i}>
            {i > 0 && <Hairline />}
            <div className="flex items-center gap-3 py-3.5">
              <Skeleton className="size-9 shrink-0 rounded-lg" />
              <Skeleton className="h-4 flex-1" />
              <Skeleton className="h-8 w-20 shrink-0 rounded-md" />
            </div>
          </div>
        ))
      ) : list.length === 0 ? (
        <p className="py-6 text-center text-sm text-muted-foreground">No active sessions.</p>
      ) : (
        list.map((s, i) => {
          const { label, icon: Icon } = deviceOf(s.user_agent);
          return (
            <div key={s.id ?? i}>
              {i > 0 && <Hairline />}
              {/* Device and action stack below `sm` — side by side, both the device label and
                  the ip/last-seen meta were being clipped to an ellipsis on a phone. */}
              <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2.5 py-3.5">
                <div className="flex min-w-0 items-start gap-3">
                  <span className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-main-surface text-foreground">
                    <Icon className="size-4.5" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium text-foreground">{label}</p>
                    <p className="break-words text-xs leading-snug text-muted-foreground">{metaOf(s, name)}</p>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5 pl-12 sm:pl-0">
                  {s.current ? (
                    <>
                      <Pill>This device</Pill>
                      <TipAnchor anchor="settings.sessions.this-device" />
                    </>
                  ) : (
                    <>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busy}
                        onClick={() => s.id && onRevoke(s.id)}
                        className="border-main-accent-t4/40 text-main-accent-t4 hover:text-main-accent-t4"
                      >
                        Revoke
                      </Button>
                      <TipAnchor anchor="settings.sessions.revoke" />
                    </>
                  )}
                </div>
              </div>
            </div>
          );
        })
      )}

      {!loading && hasOthers && (
        <div className="mb-2 mt-3 flex items-center gap-1.5">
          <Button
            variant="outline"
            disabled={busy}
            onClick={onRevokeOthers}
            className="w-full border-main-accent-t4/40 text-main-accent-t4 hover:text-main-accent-t4"
          >
            {busy && <Loader2 className="mr-1.5 size-4 animate-spin" />} Sign out all other devices
          </Button>
          <TipAnchor anchor="settings.sessions.revoke-others" />
        </div>
      )}
    </ListCard>
  );
}

/** Desktop card header. */
function SectionHeader({ title, sub }: { title: string; sub: string }) {
  return (
    <header className="mb-4">
      <h2 className="text-sm font-semibold tracking-normal text-foreground">{title}</h2>
      <p className="text-xs text-muted-foreground">{sub}</p>
    </header>
  );
}

function Field({ label, trailing, children }: { label: string; trailing?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex min-w-65 flex-1 flex-col">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-xs text-muted-foreground">{label}</span>
        {trailing}
      </div>
      {children}
    </div>
  );
}

function FieldSkeleton() {
  return <Skeleton className="h-9 w-full rounded-md" />;
}

function VerifiedTag() {
  return (
    <span className="inline-flex items-center gap-1 text-xs font-semibold text-main-accent-t1">
      <BadgeCheck className="size-3" /> Verified
    </span>
  );
}

function ThemedSelect({
  value,
  onChange,
  options,
  placeholder,
  error,
}: {
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
  placeholder: string;
  error?: string;
}) {
  return (
    <div className="min-w-0 flex-1">
      <Select value={value || undefined} onValueChange={onChange}>
        <SelectTrigger className="w-full border-border bg-main-surface">
          {/* Not `SelectValue`: the uikit's renders the raw stored value, so the trigger
              read "en" / "Asia/Ho_Chi_Minh" instead of the option label the design shows. */}
          <span className={cn("truncate", !value && "text-muted-foreground")}>{value ? labelOf(options, value) : placeholder}</span>
        </SelectTrigger>
        <SelectContent>
          {options.map((o) => (
            <SelectItem key={o.value} value={o.value}>
              {o.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {error && <p className="mt-1 text-xs text-destructive">{error}</p>}
    </div>
  );
}

// Best-effort device label from the captured User-Agent — enough to recognise a device,
// not a parser. Mobile UAs get the phone glyph.
function deviceOf(ua: string | undefined): { label: string; icon: LucideIcon } {
  const u = (ua ?? "").toLowerCase();
  if (!u) return { label: "Unknown device", icon: Laptop };
  const mobile = /iphone|android|mobile/.test(u);
  const browser = /edg/.test(u) ? "Edge" : /firefox|fxios/.test(u) ? "Firefox" : /chrome|crios/.test(u) ? "Chrome" : /safari/.test(u) ? "Safari" : "Browser";
  const os = /iphone|ipad|ios|crios|fxios/.test(u) ? "iOS" : /android/.test(u) ? "Android" : /mac os|macintosh/.test(u) ? "macOS" : /windows/.test(u) ? "Windows" : /linux/.test(u) ? "Linux" : "device";
  return { label: `${browser} · ${os}`, icon: mobile ? Smartphone : Laptop };
}

function metaOf(s: Session, name: string): string {
  const ip = (s.ip ?? "").trim();
  const when = s.current ? "Active now" : lastSeen(s.last_seen);
  return [name, ip || null, when].filter(Boolean).join(" · ");
}

function lastSeen(value: number | string | undefined): string {
  const secs = Number(value ?? 0);
  if (!Number.isFinite(secs) || secs <= 0) return "active recently";
  const diff = Date.now() - secs * 1000;
  if (diff < 60_000) return "active now";
  const mins = Math.floor(diff / 60_000);
  if (mins < 60) return `last active ${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `last active ${hrs}h ago`;
  return `last active ${Math.floor(hrs / 24)}d ago`;
}

/** Normalise the stored email for display via the `Email` TypeObject. */
function formatEmail(raw?: string | null): string {
  if (!raw) return "";
  const e = Email.parseInput(raw) ?? Email.fromUnsafe(raw);
  return Email.raw(e);
}

function formatPhone(raw: string): string {
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

// Google's four brand hexes are fixed by their identity guidelines and are passed to the
// SVG `fill` attribute, which takes a value rather than a class — so they stay literal and
// deliberately do not follow the theme.
function GoogleMark() {
  return (
    <svg viewBox="0 0 24 24" className="size-4.5" aria-hidden="true">
      <path fill="#4285F4" d="M23.52 12.27c0-.82-.07-1.6-.2-2.36H12v4.47h6.47a5.53 5.53 0 0 1-2.4 3.63v3h3.88c2.27-2.09 3.57-5.17 3.57-8.74Z" />
      <path fill="#34A853" d="M12 24c3.24 0 5.96-1.08 7.95-2.91l-3.88-3.01c-1.08.72-2.45 1.15-4.07 1.15-3.13 0-5.78-2.11-6.73-4.96H1.27v3.1A12 12 0 0 0 12 24Z" />
      <path fill="#FBBC05" d="M5.27 14.27a7.2 7.2 0 0 1 0-4.54v-3.1H1.27a12 12 0 0 0 0 10.74l4-3.1Z" />
      <path fill="#EA4335" d="M12 4.77c1.76 0 3.35.61 4.6 1.8l3.43-3.43A11.97 11.97 0 0 0 12 0 12 12 0 0 0 1.27 6.63l4 3.1C6.22 6.88 8.87 4.77 12 4.77Z" />
    </svg>
  );
}

/**
 * Delivery preferences. Both master channels are opt-out and may be off at once —
 * "stop contacting me" is a supported state, so nothing here keeps one of them on.
 *
 * Every write returns the full snapshot, so state is replaced from the response
 * rather than patched locally; that keeps the per-topic email toggles honest when
 * the master email switch turns them all moot.
 */
function NotificationsSection() {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [writeError, setWriteError] = useState<string | null>(null);

  // Every write answers with the whole new matrix and publishes it into the cache, so the
  // toggles below stay in step without this section holding a second copy of the state.
  const read = useResource(notificationSettingsResource);
  const settings = read.data ?? null;
  // Only a read that has actually failed reports — while it is still in flight there is
  // nothing wrong, and the skeleton switches below are the right thing to show.
  const error = writeError ?? (settings || !read.error ? null : (read.error.message || "could not load notification settings"));

  async function run(fn: () => Promise<NotificationSettings>) {
    setBusy(true);
    setWriteError(null);
    try {
      await fn();
      // Switching the in-app channel changes what the badge should read, and the
      // sidebar has no other reason to refetch.
      void refreshUnreadCount();
    } catch (e) {
      setWriteError(e instanceof Error ? e.message : "could not save");
    } finally {
      setBusy(false);
    }
  }

  if (error && !settings) {
    return <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>;
  }

  return (
    <div className="flex flex-col gap-4 lg:gap-4.5">
      <ListCard className="lg:px-5.5">
        <ListCardTitle sub="Choose where notifications reach you. Both can be off.">Delivery</ListCardTitle>
        <Hairline />
        <Row>
          <RowLabel title={t("ui.inYourCabinet")} sub="On by default. Turn this off and notifications stop appearing in your cabinet." />
          {settings ? (
            <Switch
              checked={settings.in_app_enabled}
              disabled={busy}
              onCheckedChange={(v) => void run(() => setChannelEnabled("in_app", v))}
              aria-label="In-app notifications"
            />
          ) : (
            <Skeleton className="h-5 w-9 shrink-0 rounded-full" />
          )}
        </Row>
        <Hairline />
        <Row>
          <RowLabel
            title={t("ui.email")}
            sub={settings ? (settings.email_verified ? `Sent to ${settings.email} · verified` : `${settings.email} · unverified, so email is not sent`) : undefined}
          />
          {settings ? (
            <Switch
              checked={settings.email_enabled}
              disabled={busy || !settings.email_verified}
              onCheckedChange={(v) => void run(() => setChannelEnabled("email", v))}
              aria-label="Email notifications"
            />
          ) : (
            <Skeleton className="h-5 w-9 shrink-0 rounded-full" />
          )}
        </Row>
      </ListCard>

      <ListCard className="lg:px-5.5">
        <div className="flex items-start justify-between gap-4">
          <ListCardTitle sub="Email a copy for these. Unsubscribing is one click — no confirmation step.">What you follow</ListCardTitle>
          <p className="shrink-0 pt-3 text-xs font-semibold uppercase tracking-widest text-muted-foreground">Email</p>
        </div>
        {settings
          ? settings.topics.map((t) => (
              <div key={t.topic}>
                <Hairline />
                {/* Wraps rather than switching at a breakpoint: the controls drop under
                    the label only when they genuinely do not fit, so the row is correct at
                    every width instead of at two. The `sm:` variants this replaced were
                    rendering as a permanent centred column — see the PR for detail. */}
                <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2.5 py-3.5">
                  <RowLabel title={t.label} sub={t.description} />
                  <div className="flex shrink-0 items-center gap-3">
                    {/* Kept hand-written: at 28px it is shorter than uikit's smallest Button, and
                        growing it would push the switch beside it out of the row. */}
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void run(() => setTopicSubscription(t.topic, !t.subscribed, t.email_enabled))}
                      className={cn(
                        "rounded-lg px-3 py-1.5 text-xs font-medium outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40",
                        t.subscribed ? "border border-border/60 text-foreground hover:bg-foreground/5" : "border border-main-accent-t1/50 text-main-accent-t1 hover:bg-main-accent-t1/10",
                      )}
                    >
                      {t.subscribed ? "Following" : "Follow"}
                    </button>
                    <Switch
                      checked={t.subscribed && t.email_enabled && settings.email_enabled}
                      disabled={busy || !t.subscribed || !settings.email_enabled}
                      onCheckedChange={(v) => void run(() => setTopicSubscription(t.topic, true, v))}
                      aria-label={`Email copy for ${t.label}`}
                    />
                  </div>
                </div>
              </div>
            ))
          : [0, 1, 2].map((i) => (
              <div key={i}>
                <Hairline />
                <Row>
                  <Skeleton className="h-4 w-32" />
                  <Skeleton className="h-5 w-9 shrink-0 rounded-full" />
                </Row>
              </div>
            ))}
      </ListCard>

      {error && <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>}
      <p className="text-xs leading-relaxed text-muted-foreground">
        Turn a channel off and we stop sending on it. Turn both off and we stop notifying you altogether — updates still live on the fund pages whenever you want them.
      </p>
    </div>
  );
}
