"use client";

// The desktop section rail (Figma `cabinet/settings`), in the two groups the surface is
// organised by. Each group has an eyebrow and a one-line caption saying what kind of thing
// lives under it — the caption is what answers "where do I change X" before a row is read.

import { useT } from "@evinvest/i18n/react";

import { Bell, type LucideIcon, Monitor, Shield, SlidersHorizontal, UserRound } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { GROUPS, type Section } from "@/views/settings/lib/sections";

// Module-scope, so the labels are catalogue keys rather than finished English — the rail
// resolves them at render.
const ROWS: Record<Section, { labelKey: string; icon: LucideIcon }> = {
  preferences: { labelKey: "settings.nav.preferences", icon: SlidersHorizontal },
  notifications: { labelKey: "nav.notifications", icon: Bell },
  personal: { labelKey: "settings.nav.personal", icon: UserRound },
  security: { labelKey: "ui.security", icon: Shield },
  sessions: { labelKey: "ui.sessionsDevices", icon: Monitor },
};

const GROUP_COPY = {
  cabinet: { labelKey: "settings.group.cabinet", subKey: "settings.group.cabinetSub" },
  profile: { labelKey: "ui.profile", subKey: "settings.group.profileSub" },
} as const;

export function SettingsRail({ section, onSelect }: { section: Section; onSelect: (id: Section) => void }) {
  const t = useT();
  return (
    // Hand-written rail — uikit has no section-nav component, so the items carry their own focus ring.
    <nav aria-label={t("settings.a11y.sections")} className="flex w-60 shrink-0 flex-col gap-5">
      {GROUPS.map((group) => {
        const copy = GROUP_COPY[group.id];
        return (
          <div key={group.id} className="flex flex-col gap-1">
            {/* The same eyebrow the sidebar's groups use, so the two rails read as one system. */}
            <div className="mb-1 flex flex-col gap-0.5 px-3">
              <p className="text-xs font-semibold uppercase tracking-widest text-ink-soft">{t(copy.labelKey)}</p>
              <p className="text-xs leading-snug text-ink-soft">{t(copy.subKey)}</p>
            </div>
            {group.sections.map((id) => {
              const { labelKey, icon: Icon } = ROWS[id];
              const active = section === id;
              return (
                <button
                  key={id}
                  type="button"
                  aria-current={active ? "page" : undefined}
                  onClick={() => onSelect(id)}
                  className={cn(
                    "flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring",
                    active ? "bg-accent-debug/15 font-semibold text-accent-debug" : "text-ink hover:bg-ink/5",
                  )}
                >
                  <Icon className="size-4.5 shrink-0" />
                  {/* i18n-max: 20 — a 240px rail row less the icon and padding. */}
                  <span className="truncate">{t(labelKey)}</span>
                </button>
              );
            })}
          </div>
        );
      })}
    </nav>
  );
}
