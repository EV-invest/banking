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
import type { Translate } from "@evinvest/i18n";

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
// The label is a catalogue key rather than the raw `legal_name`, which is what the
// English message used to interpolate — a wire identifier reads badly in any language.
const NAME_FIELD_LABEL: Readonly<Record<keyof typeof NAME_MAX, string>> = Object.freeze({
  legal_name: "profile.legalName",
  preferred_name: "profile.preferredName",
  nationality: "profile.nationality",
  tax_residence: "profile.taxResidence",
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
      errors[field] = isNameField(field)
        ? t(issue.message, { field: t(NAME_FIELD_LABEL[field]), n: NAME_MAX[field] })
        : t(issue.message);
    }
  }
  return errors;
}
