// The settings surface's map: which sections exist, which group each belongs to, and the
// personal-details fields the Profile group edits. Pure, so the server page can parse a
// `?section=` and the two breakpoints can render the same list from one declaration.

import type { Translate } from "@evinvest/i18n";

import type { TipKey } from "@/shared/tips";

import type { Form } from "@/views/settings/lib/form";

/**
 * Three groups. "Cabinet" is how the cabinet behaves for the reader — what it displays and
 * how it reaches them; "Profile" is who the reader is and how their account is protected.
 * The split exists because the two used to share one "General" pane and nobody could say
 * where a given thing was changed. "Help" is neither: what the fund has published and how
 * to reach a person (#385) — the cabinet has no footer to carry those, so they live here.
 */
export const SECTIONS = ["preferences", "notifications", "personal", "security", "sessions", "documents"] as const;
export type Section = (typeof SECTIONS)[number];
export const DEFAULT_SECTION: Section = "preferences";

export const GROUPS: ReadonlyArray<{ id: "cabinet" | "profile" | "help"; sections: readonly Section[] }> = [
  { id: "cabinet", sections: ["preferences", "notifications"] },
  { id: "profile", sections: ["personal", "security", "sessions"] },
  { id: "help", sections: ["documents"] },
];

/** A `?section=` value as a section, or the default for anything that is not one. */
export function sectionFrom(raw: string | undefined): Section {
  return (SECTIONS as readonly string[]).includes(raw ?? "") ? (raw as Section) : DEFAULT_SECTION;
}

/**
 * The sections the mobile stack pushes as their own screen. Preferences and Security are
 * root-screen cards there, so a deep link to either lands on the root.
 */
export const PUSHABLE = ["personal", "notifications", "sessions", "documents"] as const;
export type Pushable = (typeof PUSHABLE)[number];
export function pushableOf(section: Section): Pushable | null {
  return (PUSHABLE as readonly string[]).includes(section) ? (section as Pushable) : null;
}

/** The sections the one profile form is saved from — the heading's Save button follows these. */
export const EDITING: readonly Section[] = ["preferences", "personal"];

/**
 * The identity fields, in the order both breakpoints render them. Labels and hints are
 * resolved at render against the reader's `t`; `tip` is a compile-time anchor into the tip catalog.
 * The hint is the sentence under the control that says what the field is for — the
 * questions support used to answer one at a time.
 */
export const PERSONAL: ReadonlyArray<{ key: keyof Form; label: (t: Translate) => string; hint?: (t: Translate) => string; tip?: TipKey }> = [
  { key: "legal_name", label: (t) => t("profile.legalName", "Legal name"), tip: "profile.field.legal-name" },
  { key: "preferred_name", label: (t) => t("profile.preferredName", "Preferred name"), hint: (t) => t("settings.hint.preferredName", "How the cabinet and our emails address you.") },
  { key: "phone", label: (t) => t("profile.phoneNumber", "Phone number"), hint: (t) => t("settings.hint.phone", "Used to reach you about your account.") },
  { key: "date_of_birth", label: (t) => t("profile.dateOfBirth", "Date of birth"), hint: (t) => t("settings.hint.dateOfBirth", "As on your identity document.") },
  { key: "nationality", label: (t) => t("profile.nationality", "Nationality"), tip: "profile.field.nationality" },
  { key: "tax_residence", label: (t) => t("profile.taxResidence", "Tax residence"), tip: "profile.field.tax-residence" },
  { key: "residential_address", label: (t) => t("profile.residentialAddress", "Residential address"), hint: (t) => t("settings.hint.address", "Your current home address.") },
];
