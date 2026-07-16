"use client";

import { BadgeCheck, Check, Laptop, Loader2, type LucideIcon, Monitor, Shield, Smartphone, User } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";

import { Badge, Button, Input, Select, SelectContent, SelectItem, SelectTrigger, SelectValue, Skeleton } from "@evinvest/uikit";

import { fetchSessions, revokeSession } from "@/entities/session/api/sessions-client";
import { fetchProfile, saveProfile } from "@/entities/user/api/profile-client";
import { publishProfile } from "@/entities/user/model/profile-store";
import type { Session, UpdateProfileRequest, UserProfile } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { displayName } from "@/views/settings/lib/format";

const CARD = "rounded-[14px] border border-border bg-main-card";

// The investor settings surface (Figma `cabinet/settings`, node 481:250), wired to the
// backend over the BFF. General edits the same core user record as the Profile page
// (full-replace, so it carries every editable field); Security states the real auth model
// (Google-managed) and surfaces live sessions; Sessions & devices lists and revokes the
// real refresh-token families at the hub. Auth is Google-OAuth-only, so there are no
// password/2FA/biometric controls — that part of the mock does not map to reality.

const EDITABLE = ["legal_name", "preferred_name", "phone", "date_of_birth", "nationality", "tax_residence", "residential_address", "language", "base_currency", "timezone"] as const;
type Form = Record<(typeof EDITABLE)[number], string>;

function formFrom(p: UserProfile): Form {
  return Object.fromEntries(EDITABLE.map((k) => [k, p[k] ?? ""])) as Form;
}

type Section = "general" | "security" | "sessions";

const NAV: { id: Section; label: string; icon: LucideIcon }[] = [
  { id: "general", label: "General", icon: User },
  { id: "security", label: "Security", icon: Shield },
  { id: "sessions", label: "Sessions & devices", icon: Monitor },
];

export function SettingsView() {
  const [section, setSection] = useState<Section>("general");

  const [profile, setProfile] = useState<UserProfile | null | undefined>(undefined);
  const [form, setForm] = useState<Form | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const savedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [sessions, setSessions] = useState<Session[] | undefined>(undefined);
  const [sessionsError, setSessionsError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const loadSessions = useCallback(() => {
    fetchSessions()
      .then((s) => {
        setSessions(s);
        setSessionsError(null);
      })
      .catch((e: Error) => {
        setSessions([]);
        setSessionsError(e.message);
      });
  }, []);

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
    loadSessions();
  }, [loadSessions]);

  const loading = profile === undefined;
  const email = profile?.email ?? null;
  const name = (profile?.legal_name ?? "").trim() || displayName(email);
  const dirty = !!form && !!profile && EDITABLE.some((k) => form[k] !== (profile[k] ?? ""));

  function set(key: keyof Form, value: string) {
    setSaved(false);
    setForm((f) => (f ? { ...f, [key]: value } : f));
  }
  async function save() {
    if (!form || saving) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await saveProfile(form as UpdateProfileRequest);
      setProfile(updated);
      setForm(formFrom(updated));
      publishProfile(updated); // the sidebar account chip picks up the new name live
      setSaved(true);
      if (savedTimer.current) clearTimeout(savedTimer.current);
      savedTimer.current = setTimeout(() => setSaved(false), 2500);
    } catch (e) {
      setError((e as Error).message);
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
    setSessionsError(null);
    try {
      await revokeSession(id);
      loadSessions();
    } catch (e) {
      setSessionsError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  async function revokeOthers() {
    const others = (sessions ?? []).filter((s) => !s.current && s.id);
    if (!others.length) return;
    setBusy(true);
    setSessionsError(null);
    try {
      for (const s of others) await revokeSession(s.id!);
      loadSessions();
    } catch (e) {
      setSessionsError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-6 px-8 pb-10 pt-6">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <h1 className="font-sans text-2xl font-semibold text-foreground">Settings</h1>
          <p className="text-[13px] text-muted-foreground">Manage your account, security and access</p>
        </div>
        {section === "general" && (
          <div className="flex shrink-0 items-center gap-3">
            {saved && (
              <span className="inline-flex items-center gap-1 text-[13px] font-medium text-main-accent-t2">
                <Check className="size-4" /> Saved
              </span>
            )}
            <button
              type="button"
              onClick={save}
              disabled={loading || saving || !dirty}
              className="inline-flex items-center gap-1.5 rounded-lg bg-main-accent-t1 px-4 py-[9px] text-[13px] font-semibold text-main-black transition-opacity hover:opacity-90 disabled:opacity-50"
            >
              {saving && <Loader2 className="size-4 animate-spin" />} Save changes
            </button>
          </div>
        )}
      </div>

      {error && <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>}

      <div className="flex gap-6">
        <nav aria-label="Settings sections" className="flex w-[212px] shrink-0 flex-col gap-1">
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
                  "flex items-center gap-[11px] rounded-lg px-3 py-[9px] text-sm transition-colors",
                  active ? "bg-main-accent-t1/15 font-semibold text-main-accent-t1" : "text-main-mist/90 hover:bg-foreground/[0.04]",
                )}
              >
                <Icon className="size-[18px]" />
                {item.label}
              </button>
            );
          })}
        </nav>

        <div className="min-w-0 flex-1">
          {section === "general" && <GeneralSection loading={loading} form={form} email={email} verified={!!profile?.email_verified} onChange={set} />}
          {section === "security" && <SecuritySection email={email} loading={loading} sessions={sessions} onManageSessions={() => setSection("sessions")} />}
          {section === "sessions" && (
            <SessionsSection
              sessions={sessions}
              error={sessionsError}
              busy={busy}
              name={name}
              onRevoke={revoke}
              onRevokeOthers={revokeOthers}
            />
          )}
        </div>
      </div>
    </div>
  );
}

const LANGUAGES = [
  { value: "en", label: "English" },
  { value: "vi", label: "Tiếng Việt" },
];
const CURRENCIES = [
  { value: "USD", label: "USD ($)" },
  { value: "USDT", label: "USDT" },
  { value: "EUR", label: "EUR (€)" },
];
const TIMEZONES = [
  { value: "Asia/Ho_Chi_Minh", label: "Asia / Ho Chi Minh" },
  { value: "UTC", label: "UTC" },
];

function GeneralSection({
  loading,
  form,
  email,
  verified,
  onChange,
}: {
  loading: boolean;
  form: Form | null;
  email: string | null;
  verified: boolean;
  onChange: (key: keyof Form, value: string) => void;
}) {
  const ready = !loading && !!form;
  return (
    <section className={cn(CARD, "px-6 py-[22px]")}>
      <header className="mb-5">
        <h2 className="text-[15px] font-semibold text-white">Account</h2>
        <p className="text-xs text-muted-foreground">Your contact details and preferences</p>
      </header>
      <div className="flex flex-wrap gap-[16px_18px]">
        <Field label="Legal name">
          {ready ? <Input value={form.legal_name} onChange={(e) => onChange("legal_name", e.target.value)} className="border-border bg-main-surface" /> : <FieldSkeleton />}
        </Field>
        <Field label="Preferred name">
          {ready ? <Input value={form.preferred_name} onChange={(e) => onChange("preferred_name", e.target.value)} className="border-border bg-main-surface" /> : <FieldSkeleton />}
        </Field>
        <Field label="Email address" trailing={verified ? <VerifiedTag /> : undefined}>
          {loading ? <FieldSkeleton /> : <Input value={email ?? ""} readOnly className="border-border bg-main-surface text-muted-foreground" />}
        </Field>
        <Field label="Phone number">
          {ready ? <Input value={form.phone} onChange={(e) => onChange("phone", e.target.value)} className="border-border bg-main-surface" /> : <FieldSkeleton />}
        </Field>
        <Field label="Language">
          {ready ? <ThemedSelect value={form.language} onChange={(v) => onChange("language", v)} options={LANGUAGES} placeholder="Select language" /> : <FieldSkeleton />}
        </Field>
        <Field label="Base currency">
          {ready ? <ThemedSelect value={form.base_currency} onChange={(v) => onChange("base_currency", v)} options={CURRENCIES} placeholder="Select currency" /> : <FieldSkeleton />}
        </Field>
        <Field label="Time zone">
          {ready ? <ThemedSelect value={form.timezone} onChange={(v) => onChange("timezone", v)} options={TIMEZONES} placeholder="Select time zone" /> : <FieldSkeleton />}
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
  const count = sessions?.length;
  const summary = count === undefined ? "Loading active sessions…" : count === 1 ? "1 device currently signed in" : `${count} devices currently signed in`;
  return (
    <section className={cn(CARD, "px-6 py-[22px]")}>
      <header className="mb-4">
        <h2 className="text-[15px] font-semibold text-white">Security</h2>
        <p className="text-xs text-muted-foreground">How you sign in and where your account is active</p>
      </header>
      <div className="flex items-center gap-3 rounded-xl border border-border bg-main-surface px-4 py-[14px]">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-white">
          <GoogleMark />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-[14px] text-white">Signed in with Google</p>
          {loading ? <Skeleton className="mt-1 h-3.5 w-44" /> : <p className="truncate text-[13px] text-muted-foreground">{email ?? "—"}</p>}
        </div>
        <Badge className="border-transparent bg-main-accent-t1/15 text-main-accent-t1">Connected</Badge>
      </div>
      <p className="mt-3 text-[13px] leading-relaxed text-muted-foreground">
        Your sign-in and password are managed by Google. Two-factor authentication and recovery are configured in your Google Account.
      </p>
      <SettingRow title="Sessions &amp; devices" sub={summary} first>
        <Button variant="outline" size="sm" className="border-border" onClick={onManageSessions}>
          Manage
        </Button>
      </SettingRow>
    </section>
  );
}

function SessionsSection({
  sessions,
  error,
  busy,
  name,
  onRevoke,
  onRevokeOthers,
}: {
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
    <section className={cn(CARD, "px-6 py-[22px]")}>
      <header className="mb-2">
        <h2 className="text-[15px] font-semibold text-white">Sessions &amp; devices</h2>
        <p className="text-xs text-muted-foreground">Where you&apos;re signed in — revoke anything you don&apos;t recognise</p>
      </header>

      {error && <p className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>}

      {loading ? (
        [0, 1].map((i) => (
          <SettingRow key={i} first={i === 0} leading={<Skeleton className="size-9 rounded-lg" />} title="" sub="">
            <Skeleton className="h-8 w-20 rounded-md" />
          </SettingRow>
        ))
      ) : list.length === 0 ? (
        <p className="py-6 text-center text-[13px] text-muted-foreground">No active sessions.</p>
      ) : (
        list.map((s, i) => {
          const { label, icon: Icon } = deviceOf(s.user_agent);
          return (
            <SettingRow
              key={s.id ?? i}
              first={i === 0}
              leading={
                <span className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-main-surface text-main-mist/90">
                  <Icon className="size-[18px]" />
                </span>
              }
              title={s.current ? `${label} · this device` : label}
              sub={metaOf(s, name)}
            >
              {s.current ? (
                <span className="flex items-center gap-1.5">
                  <Badge className="border-transparent bg-main-accent-t1/15 text-main-accent-t1">This device</Badge>
                  <TipAnchor anchor="settings.sessions.this-device" />
                </span>
              ) : (
                <span className="flex items-center gap-1.5">
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
                </span>
              )}
            </SettingRow>
          );
        })
      )}

      {!loading && hasOthers && (
        <div className="mt-4 flex items-center justify-end gap-1.5">
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
    </section>
  );
}

function Field({ label, trailing, children }: { label: string; trailing?: ReactNode; children: ReactNode }) {
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

function FieldSkeleton() {
  return <Skeleton className="h-9 w-full rounded-md" />;
}

function VerifiedTag() {
  return (
    <span className="inline-flex items-center gap-1 text-[11px] font-semibold text-main-accent-t1">
      <BadgeCheck className="size-3" /> Verified
    </span>
  );
}

function ThemedSelect({ value, onChange, options, placeholder }: { value: string; onChange: (v: string) => void; options: { value: string; label: string }[]; placeholder: string }) {
  return (
    <Select value={value || undefined} onValueChange={onChange}>
      <SelectTrigger className="w-full border-border bg-main-surface">
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function SettingRow({
  title,
  sub,
  first,
  leading,
  children,
}: {
  title: ReactNode;
  sub?: ReactNode;
  first?: boolean;
  leading?: ReactNode;
  children: ReactNode;
}) {
  return (
    <>
      {!first && <div className="h-px bg-border" />}
      <div className="flex items-center justify-between gap-4 py-[14px]">
        <div className="flex min-w-0 items-center gap-3">
          {leading}
          <div className="min-w-0">
            <p className="truncate text-[14px] text-white">{title}</p>
            {sub && <p className="truncate text-[13px] text-muted-foreground">{sub}</p>}
          </div>
        </div>
        <div className="shrink-0">{children}</div>
      </div>
    </>
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

function GoogleMark() {
  return (
    <svg viewBox="0 0 24 24" className="size-[18px]" aria-hidden="true">
      <path fill="#4285F4" d="M23.52 12.27c0-.82-.07-1.6-.2-2.36H12v4.47h6.47a5.53 5.53 0 0 1-2.4 3.63v3h3.88c2.27-2.09 3.57-5.17 3.57-8.74Z" />
      <path fill="#34A853" d="M12 24c3.24 0 5.96-1.08 7.95-2.91l-3.88-3.01c-1.08.72-2.45 1.15-4.07 1.15-3.13 0-5.78-2.11-6.73-4.96H1.27v3.1A12 12 0 0 0 12 24Z" />
      <path fill="#FBBC05" d="M5.27 14.27a7.2 7.2 0 0 1 0-4.54v-3.1H1.27a12 12 0 0 0 0 10.74l4-3.1Z" />
      <path fill="#EA4335" d="M12 4.77c1.76 0 3.35.61 4.6 1.8l3.43-3.43A11.97 11.97 0 0 0 12 0 12 12 0 0 0 1.27 6.63l4 3.1C6.22 6.88 8.87 4.77 12 4.77Z" />
    </svg>
  );
}
