// The shape of the settings form: the profile fields this surface edits, and the option
// lists the three choice fields pick from. Pure, so the mobile row editors and the desktop
// General form work from one field set and one set of labels.

import { LOCALES, LOCALE_LABELS } from "@evinvest/i18n";

import type { UserProfile } from "@/shared/contracts";

export const EDITABLE = ["legal_name", "preferred_name", "phone", "date_of_birth", "nationality", "tax_residence", "residential_address", "language", "base_currency", "timezone"] as const;
export type Form = Record<(typeof EDITABLE)[number], string>;

export function formFrom(p: UserProfile): Form {
  return Object.fromEntries(EDITABLE.map((k) => [k, p[k] ?? ""])) as Form;
}

// Every locale the platform publishes, each labelled in its own language — a reader who
// cannot read the current language cannot read "Russian" either, which is why
// @evinvest/i18n owns the labels rather than a catalogue key. Derived from LOCALES so
// launching a sixth locale needs no edit here; the two hardcoded options this replaced
// meant the other three were unreachable from the cabinet even though every route,
// catalogue and redirect already supported them.
export const LANGUAGES = LOCALES.map((value) => ({ value, label: LOCALE_LABELS[value] }));
export const CURRENCIES = [
  { value: "USD", label: "USD ($)" },
  { value: "EUR", label: "EUR (€)" },
];
export const TIMEZONES = [
  { value: "Asia/Ho_Chi_Minh", label: "Asia / Ho Chi Minh" },
  { value: "UTC", label: "UTC" },
];

export function labelOf(options: { value: string; label: string }[], value: string): string {
  return options.find((o) => o.value === value)?.label ?? value;
}
