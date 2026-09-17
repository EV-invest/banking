"use client";

import { useT } from "@evinvest/i18n/react";
import { Check } from "lucide-react";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { Button, Spinner } from "@evinvest/uikit";

import { useCabinetHref } from "@/shared/lib/cabinet-route";
import { Link } from "@/shared/ui/cabinet-link";
import { InitialsAvatar } from "@/shared/ui/list-card";
import { MobileAppBar } from "@/shared/ui/mobile-appbar";
import { StaggerItem } from "@/shared/ui/motion";
import { PageFrame } from "@/shared/ui/page-frame";
import { displayName, initialsOfName, truncateName } from "@/views/settings/lib/format";
import { DEFAULT_SECTION, EDITING, pushableOf, type Section } from "@/views/settings/lib/sections";
import { useProfileForm } from "@/views/settings/lib/use-profile-form";
import { useSessions } from "@/views/settings/lib/use-sessions";
import { DesktopPane } from "@/views/settings/ui/desktop-pane";
import { MobileStack } from "@/views/settings/ui/mobile-stack";
import { SessionsSection } from "@/views/settings/ui/sessions-section";

// The investor settings surface, wired to the backend over the BFF. Two Figma frames,
// one component: `cabinet/mobile/settings` below `lg` — an app bar over a stack of row
// cards, with the editors pushed as their own screens (`MobileStack`) — and
// `cabinet/settings` above it, a section rail beside the pane (`DesktopPane`).
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
// Auth is Google-OAuth-only and there is no theme store, so the mock's 2FA/biometric/
// password rows have no backing here — they are left out rather than faked.
//
// The open section is in the URL (`?section=`), read by the server page and written back
// on every change (`replace`, so the rail adds no history): `/settings?section=personal`
// is the deep link the profile page sends a reader to, and a reload or a shared link
// re-renders the same section. Leaving a pushed screen on mobile is the app bar's back
// affordance, not the browser's.

export function SettingsView({ initialSection }: { initialSection: Section }) {
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

  const { profile, loading, error, form, dirty, saving, saved, fieldErrors, set, save } = useProfileForm();
  const sessions = useSessions();
  const email = profile?.email ?? null;
  const name = truncateName((profile?.legal_name ?? "").trim()) || displayName(email, t);
  const personal = { loading, form, email, verified: !!profile?.email_verified, onChange: set, fieldErrors };
  const sessionsPanel = (titled: boolean) => (
    <SessionsSection titled={titled} sessions={sessions.sessions} error={sessions.error} busy={sessions.busy} name={name} onRevoke={sessions.revoke} onRevokeOthers={sessions.revokeOthers} />
  );

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

      <MobileStack pushed={pushed} onSelect={select} personal={personal} sessions={sessionsPanel(false)} name={name} sessionList={sessions.sessions} />
      <DesktopPane section={section} onSelect={select} personal={personal} sessions={sessionsPanel(true)} sessionList={sessions.sessions} />
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
