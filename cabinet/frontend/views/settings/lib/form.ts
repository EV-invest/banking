// The shape of the settings form: the profile fields this surface edits, and the option
// lists the three choice fields pick from. Pure, so the mobile row editors and the desktop
// General form work from one field set and one set of labels.

import type { UserProfile } from "@/shared/contracts";

export const EDITABLE = ["legal_name", "preferred_name", "phone", "date_of_birth", "nationality", "tax_residence", "residential_address", "language", "base_currency", "timezone"] as const;
export type Form = Record<(typeof EDITABLE)[number], string>;

export function formFrom(p: UserProfile): Form {
  return Object.fromEntries(EDITABLE.map((k) => [k, p[k] ?? ""])) as Form;
}

export const LANGUAGES = [
  { value: "en", label: "English" },
  { value: "vi", label: "Tiếng Việt" },
];
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
