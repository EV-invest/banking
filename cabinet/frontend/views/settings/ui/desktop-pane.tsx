"use client";

import { useT } from "@evinvest/i18n/react";
import type { ReactNode } from "react";

import type { Session } from "@/shared/contracts";
import { Reveal, StaggerItem } from "@/shared/ui/motion";
import type { Section } from "@/views/settings/lib/sections";
import { DocumentsSection } from "@/views/settings/ui/documents-section";
import { SectionHeader } from "@/views/settings/ui/fields";
import { NotificationsSection } from "@/views/settings/ui/notifications-section";
import { PersonalSection, type PersonalProps } from "@/views/settings/ui/personal-section";
import { PreferencesSection } from "@/views/settings/ui/preferences-section";
import { SecuritySection } from "@/views/settings/ui/security-section";
import { SettingsRail } from "@/views/settings/ui/settings-rail";

// The desktop half of Settings (Figma `cabinet/settings`, node 481:250): the section rail
// beside the pane for the open section.
export function DesktopPane({ section, onSelect, personal, sessions, sessionList }: { section: Section; onSelect: (id: Section) => void; personal: PersonalProps; sessions: ReactNode; sessionList: Session[] | undefined }) {
  const t = useT();
  return (
    <StaggerItem className="hidden gap-6 lg:flex">
      <SettingsRail section={section} onSelect={onSelect} />

      {/* Keyed on the section, so choosing one from the rail brings its pane in
          rather than swapping it under the cursor. The rail beside it does not
          remount, which is the point — the marker slides, the pane arrives. */}
      <Reveal key={section} className="min-w-0 flex-1">
        {section === "preferences" && <PreferencesSection loading={personal.loading} form={personal.form} onChange={personal.onChange} fieldErrors={personal.fieldErrors} />}
        {section === "notifications" && (
          <div>
            {/* The section itself is shared with the mobile pushed screen, where the
                app bar titles it — the header is the desktop's alone. */}
            <SectionHeader title={t("nav.notifications")} sub={t("settings.notificationsSub")} />
            <NotificationsSection />
          </div>
        )}
        {section === "personal" && <PersonalSection {...personal} />}
        {section === "security" && <SecuritySection email={personal.email} loading={personal.loading} sessions={sessionList} onManageSessions={() => onSelect("sessions")} />}
        {section === "sessions" && sessions}
        {section === "documents" && (
          <div>
            <SectionHeader title={t("settings.documents.title")} sub={t("settings.documents.sub")} />
            <DocumentsSection />
          </div>
        )}
      </Reveal>
    </StaggerItem>
  );
}
