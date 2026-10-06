"use client";

// The desktop section rail (Figma `cabinet/settings`), in the two groups the surface is
// organised by. Each group has an eyebrow and a one-line caption saying what kind of thing
// lives under it — the caption is what answers "where do I change X" before a row is read.

import type { Translate } from "@evinvest/i18n";
import { useT } from "@evinvest/i18n/react";

import { Bell, FileText, type LucideIcon, Monitor, Shield, SlidersHorizontal, UserRound } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { SectionLabel } from "@/shared/ui/page-frame";
import { GROUPS, type Section } from "@/views/settings/lib/sections";

// Module-scope, so the labels are functions of `t` rather than finished English — the rail
// resolves them at render.
const ROWS: Record<Section, { label: (t: Translate) => string; icon: LucideIcon }> = {
  preferences: { label: (t) => t("settings.nav.preferences", "Preferences"), icon: SlidersHorizontal },
  notifications: { label: (t) => t("nav.notifications", "Notifications"), icon: Bell },
  personal: { label: (t) => t("settings.nav.personal", "Personal details"), icon: UserRound },
  security: { label: (t) => t("ui.security", "Security"), icon: Shield },
  sessions: { label: (t) => t("ui.sessionsDevices", "Sessions & devices"), icon: Monitor },
  documents: { label: (t) => t("settings.nav.documents", "Documents"), icon: FileText },
};

const GROUP_COPY: Record<(typeof GROUPS)[number]["id"], { label: (t: Translate) => string; sub: (t: Translate) => string }> = {
  cabinet: { label: (t) => t("settings.group.cabinet", "Cabinet"), sub: (t) => t("settings.group.cabinetSub", "How the cabinet looks and how it reaches you") },
  profile: { label: (t) => t("ui.profile", "Profile"), sub: (t) => t("settings.group.profileSub", "Who you are and how your account is protected") },
  help: { label: (t) => t("settings.group.help", "Help"), sub: (t) => t("settings.group.helpSub", "What the fund has published, and how to reach a person") },
};

export function SettingsRail({ section, onSelect }: { section: Section; onSelect: (id: Section) => void }) {
  const t = useT();
  return (
    // Hand-written rail — uikit has no section-nav component, so the items carry their own focus ring.
    <nav aria-label={t("settings.a11y.sections", "Settings sections")} className="flex w-60 shrink-0 flex-col gap-5">
      {GROUPS.map((group) => {
        const copy = GROUP_COPY[group.id];
        return (
          <div key={group.id} className="flex flex-col gap-1">
            {/* The same eyebrow the sidebar's groups use, so the two rails read as one system. */}
            <div className="mb-1 flex flex-col gap-0.5 px-3">
              <SectionLabel>{copy.label(t)}</SectionLabel>
              <p className="text-xs leading-snug text-ink-soft">{copy.sub(t)}</p>
            </div>
            {group.sections.map((id) => {
              const { label, icon: Icon } = ROWS[id];
              const active = section === id;
              return (
                <button
                  key={id}
                  type="button"
                  aria-current={active ? "page" : undefined}
                  onClick={() => onSelect(id)}
                  className={cn(
                    "flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring",
                    active ? "bg-primary-ink/15 font-semibold text-primary-ink" : "text-ink hover:bg-ink/5",
                  )}
                >
                  <Icon className="size-4.5 shrink-0" />
                  {/* i18n-max: 20 — a 240px rail row less the icon and padding. */}
                  <span className="truncate">{label(t)}</span>
                </button>
              );
            })}
          </div>
        );
      })}
    </nav>
  );
}
