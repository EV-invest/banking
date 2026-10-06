// Zod schema for the 10 editable profile fields. Mirrors the server-side
// parse-don't-validate rules in concierge `domain/src/users.rs` so a
// malformed value fails fast on the client with a field-level error.
// An empty string clears the field (the wire contract's full-replace
// semantics); validation only runs against non-empty values.
//
// Every `.max()` / `.refine()` message here is a **catalogue key, not prose**.
// Zod bakes its messages in when the schema is built, which happens once at
// module load — long before any component holds a translator — so a schema that
// carried English would be English in every locale. Rebuilding the schema per
// render to close that gap would rebuild ten Zod rules on every keystroke, so
// the codes travel out through `validateProfileForm` instead, which already
// flattens the issues and is the one place a translator is in reach.
import { z } from "zod";
import { Email, PhoneNumber } from "@evinvest/types";
import type { MessageValues, Translate } from "@evinvest/i18n";

import { wordFor } from "@/shared/lib/wire-words";

const NAME_MAX = Object.freeze({
  legal_name: 256,
  preferred_name: 64,
  nationality: 64,
  tax_residence: 64,
} as const);

// Letters (any script), spaces, hyphen, apostrophe, period.
const NAME_RE = /^[\p{L} \-'.]+$/u;

function nameRule(max: number) {
  return z
    .string()
    .trim()
    .max(max, "err.field.maxLength")
    .refine((v) => !v || NAME_RE.test(v), "err.field.nameChars")
    .refine((v) => !v || (v.match(/\p{L}/gu)?.length ?? 0) >= 2, "err.field.minLetters");
}

function phoneRule() {
  return z
    .string()
    .trim()
    .max(32, "err.phone.maxLength")
    .refine((v) => !v || PhoneNumber.parseInput(v) !== undefined, "err.phone.invalid");
}

function emailRule() {
  return z
    .string()
    .trim()
    .max(320, "err.email.maxLength")
    .refine((v) => !v || Email.parseInput(v) !== undefined, "err.email.invalid");
}

function dateOfBirthRule() {
  return z
    .string()
    .trim()
    .refine((v) => !v || /^\d{4}-\d{2}-\d{2}$/.test(v), "err.dob.format")
    .refine(
      (v) => {
        if (!v) return true;
        const y = Number(v.slice(0, 4));
        const m = Number(v.slice(5, 7));
        const d = Number(v.slice(8, 10));
        if (y < 1900 || y > 2100) return false;
        const date = new Date(y, m - 1, d);
        return date.getFullYear() === y && date.getMonth() === m - 1 && date.getDate() === d;
      },
      "err.dob.range",
    );
}

function addressRule() {
  return z
    .string()
    .trim()
    .max(256, "err.address.maxLength")
    .refine(
      (v) => !v || ![...v].some((c) => c.charCodeAt(0) < 0x20 || c === "" || (c.charCodeAt(0) >= 0x80 && c.charCodeAt(0) <= 0x9F)),
      "err.address.controlChars",
    );
}

function languageRule() {
  return z
    .string()
    .trim()
    .max(16, "err.language.maxLength")
    .refine((v) => !v || /^[a-zA-Z]{2,3}([-_][a-zA-Z0-9]{2,8})*$/.test(v), "err.language.format");
}

function currencyRule() {
  return z
    .string()
    .trim()
    .max(3, "err.currency.format")
    .refine((v) => !v || /^[a-zA-Z]{3}$/.test(v), "err.currency.format");
}

function timezoneRule() {
  const IANA = [
    "Africa",
    "America",
    "Antarctica",
    "Arctic",
    "Asia",
    "Atlantic",
    "Australia",
    "Etc",
    "Europe",
    "Indian",
    "Pacific",
  ];
  return z
    .string()
    .trim()
    .max(64, "err.timezone.maxLength")
    .refine(
      (v) =>
        !v ||
        v === "UTC" ||
        v === "GMT" ||
        IANA.some((area) => v.startsWith(`${area}/`)),
      "err.timezone.format",
    );
}

export const profileEditableSchema = z.object({
  legal_name: nameRule(NAME_MAX.legal_name),
  preferred_name: nameRule(NAME_MAX.preferred_name),
  phone: phoneRule(),
  date_of_birth: dateOfBirthRule(),
  nationality: nameRule(NAME_MAX.nationality),
  tax_residence: nameRule(NAME_MAX.tax_residence),
  residential_address: addressRule(),
  language: languageRule(),
  base_currency: currencyRule(),
  timezone: timezoneRule(),
});

export type ProfileEditable = z.infer<typeof profileEditableSchema>;

// The four name rules are the only ones whose message names its field and its limit.
// The label is the field's own name rather than the raw `legal_name`, which is what the
// English message used to interpolate — a wire identifier reads badly in any language.
const nameFieldLabels = (t: Translate): Record<keyof typeof NAME_MAX, string> => ({
  legal_name: t("profile.legalName", "Legal name"),
  preferred_name: t("profile.preferredName", "Preferred name"),
  nationality: t("profile.nationality", "Nationality"),
  tax_residence: t("profile.taxResidence", "Tax residence"),
});

// Every message key the rules above can raise, in words. A message Zod raises on its own
// (a type mismatch) is not in here and passes through as Zod's English.
const issueWords = (t: Translate, values?: MessageValues): Readonly<Record<string, string>> => ({
  "err.field.maxLength": t("err.field.maxLength", "{field} must be at most {n, plural, one {# character} other {# characters}}", values),
  "err.field.nameChars": t("err.field.nameChars", "{field} may only contain letters, spaces, hyphens, apostrophes, and periods", values),
  "err.field.minLetters": t("err.field.minLetters", "{field} must contain at least 2 letters", values),
  "err.phone.maxLength": t("err.phone.maxLength", "Phone number must be at most 32 characters"),
  "err.phone.invalid": t("err.phone.invalid", "Enter a valid phone number starting with + or country code"),
  "err.email.maxLength": t("err.email.maxLength", "Email address must be at most 320 characters"),
  "err.email.invalid": t("err.email.invalid", "Enter a valid email address"),
  "err.dob.format": t("err.dob.format", "Date of birth must be a valid YYYY-MM-DD date"),
  "err.dob.range": t("err.dob.range", "Date of birth must be a real date between 1900 and 2100"),
  "err.address.maxLength": t("err.address.maxLength", "Residential address must be at most 256 characters"),
  "err.address.controlChars": t("err.address.controlChars", "Residential address must not contain control characters"),
  "err.language.maxLength": t("err.language.maxLength", "Language must be at most 16 characters"),
  "err.language.format": t("err.language.format", "Language must be a BCP 47 code such as 'en' or 'en-US'"),
  "err.currency.format": t("err.currency.format", "Base currency must be a 3-letter code such as 'USD'"),
  "err.timezone.maxLength": t("err.timezone.maxLength", "Time zone must be at most 64 characters"),
  "err.timezone.format": t("err.timezone.format", "Time zone must be 'UTC', 'GMT', or an IANA name such as 'Asia/Ho_Chi_Minh'"),
});

const isNameField = (field: string): field is keyof typeof NAME_MAX => field in NAME_MAX;

/** Validate the form and return a flat `Record<field, error>` — empty = valid.
 *  Takes the translator because the schema's messages are catalogue keys; this is the
 *  one seam between Zod's module-load rules and the reader's language. */
export function validateProfileForm(
  form: Record<string, string>,
  t: Translate,
): Record<string, string> {
  const result = profileEditableSchema.safeParse(form);
  if (result.success) return {};
  const errors: Record<string, string> = {};
  for (const issue of result.error.issues) {
    const field = issue.path[0] as string;
    // Keep the first error per field.
    if (!errors[field]) {
      const values = isNameField(field) ? { field: nameFieldLabels(t)[field], n: NAME_MAX[field] } : undefined;
      errors[field] = wordFor(issueWords(t, values), issue.message) ?? issue.message;
    }
  }
  return errors;
}
