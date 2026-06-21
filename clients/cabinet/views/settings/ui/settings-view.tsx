"use client";

import { BadgeCheck, Laptop, type LucideIcon, Monitor, Shield, Smartphone, User } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";

import { Badge, Button, Input, Select, SelectContent, SelectItem, SelectTrigger, SelectValue, Skeleton, Switch } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { displayName } from "@/views/settings/lib/format";

const CARD = "rounded-[14px] border border-border bg-main-card";

// The investor settings surface (Figma `cabinet/settings`, node 481:250). Only the user's
// name + email are real — derived from the BFF session, like the sidebar AccountChip. Every
// other value (phone, language, security toggles, sessions) is an honest, styled scaffold:
// there is no settings/security/sessions API yet, so these read as the design intends rather
// than as fabricated live state. The toggles/active-section are local UI state only.

type Section = "general" | "security" | "sessions";

const NAV: { id: Section; label: string; icon: LucideIcon }[] = [
  { id: "general", label: "General", icon: User },
  { id: "security", label: "Security", icon: Shield },
  { id: "sessions", label: "Sessions & devices", icon: Monitor },
];

export function SettingsView() {
  const [email, setEmail] = useState<string | null | undefined>(undefined);
  const [section, setSection] = useState<Section>("general");
  const [twoFactor, setTwoFactor] = useState(true);
  const [biometric, setBiometric] = useState(false);

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

  return (
    <div className="flex flex-col gap-6 px-8 pb-10 pt-6">
      {/* topbar */}
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <h1 className="font-sans text-2xl font-semibold text-foreground">Settings</h1>
          <p className="text-[13px] text-muted-foreground">Manage your account, security and access</p>
        </div>
        <button type="button" className="shrink-0 rounded-lg bg-main-accent-t1 px-4 py-[9px] text-[13px] font-semibold text-main-black transition-opacity hover:opacity-90">
          Save changes
        </button>
      </div>

      {/* body */}
      <div className="flex gap-6">
        {/* section nav */}
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

        {/* content */}
        <div className="min-w-0 flex-1">
          {section === "general" && <GeneralSection loading={loading} name={name} email={email} />}
          {section === "security" && (
            <SecuritySection
              twoFactor={twoFactor}
              biometric={biometric}
              onTwoFactor={setTwoFactor}
              onBiometric={setBiometric}
              onManageSessions={() => setSection("sessions")}
            />
          )}
          {section === "sessions" && <SessionsSection />}
        </div>
      </div>
    </div>
  );
}

function GeneralSection({ loading, name, email }: { loading: boolean; name: string; email: string | null | undefined }) {
  return (
    <section className={cn(CARD, "px-6 py-[22px]")}>
      <header className="mb-5">
        <h2 className="text-[15px] font-semibold text-white">Account</h2>
        <p className="text-xs text-muted-foreground">Your contact details and identifiers</p>
      </header>
      <div className="flex flex-wrap gap-[16px_18px]">
        <Field label="Full name">
          {loading ? <FieldSkeleton /> : <Input defaultValue={name} className="border-border bg-main-surface" />}
        </Field>
        <Field label="Email address" trailing={<VerifiedTag />}>
          {loading ? <FieldSkeleton /> : <Input defaultValue={email ?? "—"} className="border-border bg-main-surface" />}
        </Field>
        <Field label="Phone number">
          <Input defaultValue="+84 90 123 4567" className="border-border bg-main-surface" />
        </Field>
        <Field label="Language">
          <ThemedSelect defaultValue="en" options={[{ value: "en", label: "English" }, { value: "vi", label: "Tiếng Việt" }]} />
        </Field>
        <Field label="Base currency">
          <ThemedSelect
            defaultValue="usd"
            options={[
              { value: "usd", label: "USD ($)" },
              { value: "usdt", label: "USDT" },
              { value: "eur", label: "EUR (€)" },
            ]}
          />
        </Field>
        <Field label="Time zone">
          <ThemedSelect defaultValue="hcm" options={[{ value: "hcm", label: "Asia / Ho Chi Minh" }, { value: "utc", label: "UTC" }]} />
        </Field>
      </div>
    </section>
  );
}

function SecuritySection({
  twoFactor,
  biometric,
  onTwoFactor,
  onBiometric,
  onManageSessions,
}: {
  twoFactor: boolean;
  biometric: boolean;
  onTwoFactor: (v: boolean) => void;
  onBiometric: (v: boolean) => void;
  onManageSessions: () => void;
}) {
  return (
    <section className={cn(CARD, "px-6 py-[22px]")}>
      <header className="mb-2">
        <h2 className="text-[15px] font-semibold text-white">Security</h2>
        <p className="text-xs text-muted-foreground">Protect your account and review access</p>
      </header>
      <SettingRow title="Two-factor authentication" sub="Require a one-time code at every sign-in" first>
        <Switch checked={twoFactor} onCheckedChange={onTwoFactor} />
      </SettingRow>
      <SettingRow title="Biometric login" sub="Use Face ID or fingerprint on trusted devices">
        <Switch checked={biometric} onCheckedChange={onBiometric} />
      </SettingRow>
      <SettingRow title="Password" sub="Last changed 3 months ago">
        <Button variant="outline" size="sm" className="border-border">
          Change
        </Button>
      </SettingRow>
      <SettingRow title="Trusted sessions" sub="2 devices currently signed in">
        <Button variant="outline" size="sm" className="border-border" onClick={onManageSessions}>
          Manage
        </Button>
      </SettingRow>
    </section>
  );
}

interface SessionRow {
  device: string;
  meta: string;
  icon: LucideIcon;
  current?: boolean;
}

const SESSIONS: SessionRow[] = [
  { device: "Chrome · macOS", meta: "Quy Nhon, VN · 14.169.x · Active now", icon: Laptop, current: true },
  { device: "Safari · iPhone", meta: "Quy Nhon, VN · last active 2h ago", icon: Smartphone },
  { device: "Firefox · Windows", meta: "Hanoi, VN · last active 3d ago", icon: Laptop },
];

function SessionsSection() {
  return (
    <section className={cn(CARD, "px-6 py-[22px]")}>
      <header className="mb-2">
        <h2 className="text-[15px] font-semibold text-white">Sessions &amp; devices</h2>
        <p className="text-xs text-muted-foreground">Where you&apos;re signed in — revoke anything you don&apos;t recognise</p>
      </header>
      {SESSIONS.map((s, i) => {
        const Icon = s.icon;
        return (
          <SettingRow
            key={s.device}
            first={i === 0}
            leading={
              <span className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-main-surface text-main-mist/90">
                <Icon className="size-[18px]" />
              </span>
            }
            title={s.device}
            sub={s.meta}
          >
            {s.current ? (
              <Badge className="border-transparent bg-main-accent-t1/15 text-main-accent-t1">This device</Badge>
            ) : (
              <Button variant="outline" size="sm" className="border-main-accent-t4/40 text-main-accent-t4 hover:text-main-accent-t4">
                Revoke
              </Button>
            )}
          </SettingRow>
        );
      })}
      <div className="mt-4 flex justify-end">
        <Button variant="outline" className="w-full border-main-accent-t4/40 text-main-accent-t4 hover:text-main-accent-t4">
          Sign out all other devices
        </Button>
      </div>
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

function ThemedSelect({ defaultValue, options }: { defaultValue: string; options: { value: string; label: string }[] }) {
  return (
    <Select defaultValue={defaultValue}>
      <SelectTrigger className="w-full border-border bg-main-surface">
        <SelectValue />
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
  title: string;
  sub?: string;
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
            <p className="text-[14px] text-white">{title}</p>
            {sub && <p className="text-[13px] text-muted-foreground">{sub}</p>}
          </div>
        </div>
        <div className="shrink-0">{children}</div>
      </div>
    </>
  );
}
