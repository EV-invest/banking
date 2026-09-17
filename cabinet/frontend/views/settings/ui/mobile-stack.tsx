"use client";

import { useT } from "@evinvest/i18n/react";
import type { ReactNode } from "react";

import type { Session } from "@/shared/contracts";
import { Reveal, StaggerItem } from "@/shared/ui/motion";
import { SectionLabel } from "@/shared/ui/page-frame";
import type { Pushable, Section } from "@/views/settings/lib/sections";
import { DocumentsSection, MobileHelpCard } from "@/views/settings/ui/documents-section";
import { MobileNotificationsCard, MobileSecurityCard, PersonalDetailsCard, PreferencesCard, ProfileSummaryCard, SignOutButton } from "@/views/settings/ui/mobile-cards";
import { NotificationsSection } from "@/views/settings/ui/notifications-section";
import { type PersonalProps, PersonalStack } from "@/views/settings/ui/personal-section";

// The mobile half of Settings (Figma `cabinet/mobile/settings`, node 498:259): a stack of
// row cards at the root, with the editors pushed as their own screens.
//
// Pushing a section replaces the whole stack, so the `key` remounts the reveal and the
// new screen arrives instead of appearing. It repeats the column because a wrapper that
// did not would collapse the gap between the root cards. On the page's own first paint
// this reveal is nested inside the entrance above it and fades without travelling — one
// movement, not two (see shared/ui/motion/entrance).
export function MobileStack({ pushed, onSelect, personal, sessions, name, sessionList }: { pushed: Pushable | null; onSelect: (id: Section) => void; personal: PersonalProps; sessions: ReactNode; name: string; sessionList: Session[] | undefined }) {
  const t = useT();
  return (
    <StaggerItem className="lg:hidden">
      <Reveal key={pushed ?? "root"} className="flex flex-col gap-5">
        {pushed === "personal" ? (
          <PersonalStack {...personal} />
        ) : pushed === "sessions" ? (
          sessions
        ) : pushed === "notifications" ? (
          <NotificationsSection />
        ) : pushed === "documents" ? (
          <DocumentsSection />
        ) : (
          <>
            <MobileGroup label={t("settings.group.cabinet")}>
              <PreferencesCard loading={personal.loading} form={personal.form} fieldErrors={personal.fieldErrors} onChange={personal.onChange} />
              <MobileNotificationsCard onOpen={() => onSelect("notifications")} />
            </MobileGroup>
            <MobileGroup label={t("ui.profile")}>
              <ProfileSummaryCard loading={personal.loading} name={name} email={personal.email} verified={personal.verified} />
              <PersonalDetailsCard onOpen={() => onSelect("personal")} />
              <MobileSecurityCard loading={personal.loading} email={personal.email} sessions={sessionList} onOpenSessions={() => onSelect("sessions")} />
            </MobileGroup>
            <MobileGroup label={t("settings.group.help")}>
              <MobileHelpCard onOpen={() => onSelect("documents")} />
            </MobileGroup>
            {/* Last on the screen, under no eyebrow: leaving is not a setting of any group. */}
            <SignOutButton />
          </>
        )}
      </Reveal>
    </StaggerItem>
  );
}

/** A mobile root-screen group: the same eyebrow the desktop rail and the sidebar use, over its cards. */
function MobileGroup({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-4">
      <SectionLabel className="px-1">{label}</SectionLabel>
      {children}
    </div>
  );
}
