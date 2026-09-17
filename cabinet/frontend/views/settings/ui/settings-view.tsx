"use client";

import { Check } from "lucide-react";
import { useRouter } from "next/navigation";
import { type ReactNode, useEffect, useRef, useState } from "react";

import { Button, Spinner } from "@evinvest/uikit";

import { revokeSession, sessionsResource } from "@/entities/session/model/session-resource";
import { isLocale } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";

import { relocalise } from "@/shared/config/base-path";
import { writeLocaleCookie } from "@/shared/lib/locale-cookie";
import { profileResource, saveProfile } from "@/entities/user/model/profile-resource";
import { validateProfileForm } from "@/entities/user/model/profile-schema";
import type { UpdateProfileRequest, UserProfile } from "@/shared/contracts";
import { errorMessage } from "@/shared/lib/api-client";
import { useCabinetHref } from "@/shared/lib/cabinet-route";
import { useResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";
import { InitialsAvatar } from "@/shared/ui/list-card";
import { MobileAppBar } from "@/shared/ui/mobile-appbar";
import { Reveal, StaggerItem } from "@/shared/ui/motion";
import { Eyebrow, PageFrame } from "@/shared/ui/page-frame";
import { EDITABLE, type Form, formFrom } from "@/views/settings/lib/form";
import { displayName, initialsOfName, truncateName } from "@/views/settings/lib/format";
import { DEFAULT_SECTION, EDITING, pushableOf, type Section } from "@/views/settings/lib/sections";
import { DocumentsSection, MobileHelpCard } from "@/views/settings/ui/documents-section";
import { MobileNotificationsCard, MobileSecurityCard, PersonalDetailsCard, PreferencesCard, ProfileSummaryCard, SignOutButton } from "@/views/settings/ui/mobile-cards";
import { NotificationsSection } from "@/views/settings/ui/notifications-section";
import { PersonalSection, PersonalStack } from "@/views/settings/ui/personal-section";
import { PreferencesSection } from "@/views/settings/ui/preferences-section";
import { SectionHeader } from "@/views/settings/ui/fields";
import { SecuritySection } from "@/views/settings/ui/security-section";
import { SettingsRail } from "@/views/settings/ui/settings-rail";
import { SessionsSection } from "@/views/settings/ui/sessions-section";

// The investor settings surface, wired to the backend over the BFF. Two Figma frames,
// one component: `cabinet/mobile/settings` (node 498:259) below `lg` — an app bar over a
// stack of row cards, with the editors pushed as their own screens — and `cabinet/settings`
// (node 481:250) above it, a section rail beside the pane.
//
// Six sections in three groups. Cabinet: Preferences (language, currency, time zone) and
// Notifications (the real delivery-preference store). Profile: Personal details (the
// identity fields the fund records), Security (the real auth model — Google-managed —
// with the live session count) and Sessions & devices (the refresh-token families at the
// hub, listed and revocable). The split replaced one "General" pane that mixed how the
// cabinet behaves with who the reader is, so nobody could say where a thing was changed.
// Help: Documents and disclosures, with the support mailbox — the entry points a footer
// would carry on a site with one (#385).
//
// Preferences and Personal details edit the same core user record (full-replace, so the
// one form carries every editable field whichever pane is open). Auth is Google-OAuth-only
// and there is no theme store, so the mock's 2FA/biometric/password rows have no backing
// here — they are left out rather than faked.
//
// The open section is in the URL (`?section=`), read by the server page and written back
// on every change (`replace`, so the rail adds no history): `/settings?section=personal`
// is the deep link the profile page sends a reader to, and a reload or a shared link
// re-renders the same section. Leaving a pushed screen on mobile is the app bar's back
// affordance, not the browser's.

export function SettingsView({ initialSection }: { initialSection: Section }) {
  const locale = useLocale();
  const t = useT();
  const router = useRouter();
  const toHref = useCabinetHref();
  const [section, setSection] = useState<Section>(initialSection);
  // The URL stays the source of truth: the rail's Settings link is a navigation to the
  // same page, so this instance survives it with the old section in state while the
  // address bar already says `/settings`. Adjusted during render rather than in an
  // effect so the pane is right on the frame the new prop arrives.
  const [seen, setSeen] = useState(initialSection);
  if (initialSection !== seen) {
    setSeen(initialSection);
    setSection(initialSection);
  }
  // The mobile stack: the root screen, or the section pushed on top of it. Derived from
  // the one section state, so a deep link opens the same thing at both breakpoints.
  const pushed = pushableOf(section);
  function select(id: Section) {
    setSection(id);
    // `replace`, not `push`: the rail is a tab strip, and a history entry per tab would
    // make Back walk through every one the reader glanced at.
    router.replace(`${toHref("/settings")}?section=${id}`, { scroll: false });
  }
  function pop() {
    setSection(DEFAULT_SECTION);
    router.replace(toHref("/settings"), { scroll: false });
  }

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
  const error = saveError ?? (profile || !profileError ? null : errorMessage(profileError, t));
  const sessionsError = revokeError ?? (sessions || !sessionList.error ? null : errorMessage(sessionList.error, t));

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
  const name = truncateName((profile?.legal_name ?? "").trim()) || displayName(email, t);
  const dirty = !pristine;

  function set(key: keyof Form, value: string) {
    setSaved(false);
    setForm((f) => (f ? { ...f, [key]: value } : f));
  }
  async function save() {
    if (!form || saving) return;
    const errors = validateProfileForm(form, t);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      setSaveError(t("settings.fixHighlighted"));
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
      // Language is the one field that changes the page it was edited on. It was
      // previously stored and nothing more: the value round-tripped to the profile and
      // the interface stayed in whatever language it was already in, so the control
      // looked broken even though it worked. Applying it is two steps, and both are
      // needed — the cookie so every later entry (a bookmark, the conductor's chip, an
      // unprefixed /cabinet link) resolves to the new choice, and the navigation so the
      // page the reader is looking at is actually re-rendered from the new catalogue.
      //
      // Server-confirmed value, not the form's: if the backend normalised or rejected
      // the code, the URL must follow what was actually stored rather than what was
      // typed. `en-US` and friends are stored fine but are not routable locales, so a
      // non-locale value simply leaves the interface where it is.
      const chosen = updated.language;
      if (isLocale(chosen) && chosen !== locale) {
        writeLocaleCookie(chosen);
        // A hard navigation, not router.push: the locale lives in the root layout's
        // segment, and the catalogue is chosen there at render time. `relocalise` is
        // shared with LocaleSync so the two cannot disagree about what "the same page"
        // means — it keeps the query and hash, which a hand-rolled version dropped.
        window.location.href = relocalise(chosen, window.location);
        return;
      }
    } catch (e) {
      setSaveError(errorMessage(e, t));
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
      setRevokeError(errorMessage(e, t));
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
      setRevokeError(errorMessage(e, t));
    } finally {
      setBusy(false);
    }
  }

  const sessionsPanel = (titled: boolean) => (
    <SessionsSection titled={titled} sessions={sessions} error={sessionsError} busy={busy} name={name} onRevoke={revoke} onRevokeOthers={revokeOthers} />
  );
  const personalProps = { loading, form, email, verified: !!profile?.email_verified, onChange: set, fieldErrors };

  const appBar = (
    <MobileAppBar
      title={t(pushed ? PUSHED_TITLE[pushed] : "nav.settings")}
      onBack={pushed ? pop : undefined}
      right={
        dirty ? (
          // i18n-max: 11 — a `shrink-0` Button in the app bar, beside the truncated title.
          <Button type="button" size="sm" onClick={save} disabled={saving} className="rounded-full font-semibold">
            {saving && <Spinner aria-hidden />} {t("ui.save")}
          </Button>
        ) : pushed ? undefined : (
          // The in-cabinet account chip, on mobile: the avatar is the way to the profile
          // here the same way the header chip is on desktop.
          <Link href="/profile" aria-label={t("ui.profile")} className="shrink-0 rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring">
            <InitialsAvatar initials={initialsOfName(name, email)} className="size-8.5 text-sm" />
          </Link>
        )
      }
    />
  );
  // The desktop heading's action. Also while dirty on any other section: a language change
  // followed by a glance at Security must not leave the edit hanging with nowhere to save.
  const headingAction = (EDITING.includes(section) || dirty) && (
    <>
      {saved && (
        // i18n-max: 11
        <span className="inline-flex items-center gap-1 text-sm font-medium text-positive">
          <Check className="size-4" /> {t("ui.saved")}
        </span>
      )}
      {/* i18n-max: 20 */}
      <Button type="button" onClick={save} disabled={loading || saving || !dirty} className="rounded-lg font-semibold">
        {saving && <Spinner aria-hidden />} {t("ui.saveChanges")}
      </Button>
    </>
  );

  return (
    <PageFrame title={t("nav.settings")} description={t("settings.subtitle")} actions={headingAction || undefined} appBar={appBar}>
      {error && (
        <StaggerItem as="p" className="rounded-md border border-accent-error/40 bg-accent-error/10 px-3 py-2 text-sm text-accent-error">
          {error}
        </StaggerItem>
      )}
      {/* Not a section: "Saved" appears in answer to a click, long after the page
          arrived, and belongs to the save rather than to the screen. */}
      {saved && (
        <p className="inline-flex items-center gap-1 text-sm font-medium text-positive lg:hidden">
          <Check className="size-4" /> {t("ui.saved")}
        </p>
      )}

      {/* ── Mobile (Figma cabinet/mobile/settings) ───────────────────────── */}
      {/* Pushing a section replaces the whole stack, so the `key` remounts the reveal
          and the new screen arrives instead of appearing. It repeats the column because
          a wrapper that did not would collapse the gap between the root cards. On the
          page's own first paint this reveal is nested inside the entrance above it and
          fades without travelling — one movement, not two (see shared/ui/motion/entrance). */}
      <StaggerItem className="lg:hidden">
        <Reveal key={pushed ?? "root"} className="flex flex-col gap-5">
          {pushed === "personal" ? (
            <PersonalStack {...personalProps} />
          ) : pushed === "sessions" ? (
            sessionsPanel(false)
          ) : pushed === "notifications" ? (
            <NotificationsSection />
          ) : pushed === "documents" ? (
            <DocumentsSection />
          ) : (
            <>
              <MobileGroup label={t("settings.group.cabinet")}>
                <PreferencesCard loading={loading} form={form} fieldErrors={fieldErrors} onChange={set} />
                <MobileNotificationsCard onOpen={() => select("notifications")} />
              </MobileGroup>
              <MobileGroup label={t("ui.profile")}>
                <ProfileSummaryCard loading={loading} name={name} email={email} verified={!!profile?.email_verified} />
                <PersonalDetailsCard onOpen={() => select("personal")} />
                <MobileSecurityCard loading={loading} email={email} sessions={sessions} onOpenSessions={() => select("sessions")} />
              </MobileGroup>
              <MobileGroup label={t("settings.group.help")}>
                <MobileHelpCard onOpen={() => select("documents")} />
              </MobileGroup>
              {/* Last on the screen, under no eyebrow: leaving is not a setting of any group. */}
              <SignOutButton />
            </>
          )}
        </Reveal>
      </StaggerItem>

      {/* ── Desktop (Figma cabinet/settings) ─────────────────────────────── */}
      <StaggerItem className="hidden gap-6 lg:flex">
        <SettingsRail section={section} onSelect={select} />

        {/* Keyed on the section, so choosing one from the rail brings its pane in
            rather than swapping it under the cursor. The rail beside it does not
            remount, which is the point — the marker slides, the pane arrives. */}
        <Reveal key={section} className="min-w-0 flex-1">
          {section === "preferences" && <PreferencesSection loading={loading} form={form} onChange={set} fieldErrors={fieldErrors} />}
          {section === "notifications" && (
            <div>
              {/* The section itself is shared with the mobile pushed screen, where the
                  app bar titles it — the header is the desktop's alone. */}
              <SectionHeader title={t("nav.notifications")} sub={t("settings.notificationsSub")} />
              <NotificationsSection />
            </div>
          )}
          {section === "personal" && <PersonalSection {...personalProps} />}
          {section === "security" && <SecuritySection email={email} loading={loading} sessions={sessions} onManageSessions={() => select("sessions")} />}
          {section === "sessions" && sessionsPanel(true)}
          {section === "documents" && (
            <div>
              <SectionHeader title={t("settings.documents.title")} sub={t("settings.documents.sub")} />
              <DocumentsSection />
            </div>
          )}
        </Reveal>
      </StaggerItem>
    </PageFrame>
  );
}

// App bar titles for the pushed screens. The catalogue keys, not English — resolved at render.
const PUSHED_TITLE = {
  personal: "settings.nav.personal",
  sessions: "ui.sessionsDevices",
  notifications: "nav.notifications",
  documents: "settings.nav.documents",
} as const;

/** A mobile root-screen group: the same eyebrow the desktop rail and the sidebar use, over its cards. */
function MobileGroup({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-4">
      <Eyebrow className="px-1">{label}</Eyebrow>
      {children}
    </div>
  );
}
