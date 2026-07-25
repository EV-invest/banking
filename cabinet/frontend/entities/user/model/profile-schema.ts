// Zod schema for the 10 editable profile fields. Mirrors the server-side
// parse-don't-validate rules in concierge `domain/src/users.rs` so a
// malformed value fails fast on the client with a field-level error.
// An empty string clears the field (the wire contract's full-replace
// semantics); validation only runs against non-empty values.
import { z } from "zod";
import { PhoneNumber } from "@evinvest/types";

const NAME_MAX = Object.freeze({
  legal_name: 256,
  preferred_name: 64,
  nationality: 64,
  tax_residence: 64,
} as const);

// Letters (any script), spaces, hyphen, apostrophe, period.
const NAME_RE = /^[\p{L} \-'.]+$/u;

function nameRule(field: string, max: number) {
  return z
    .string()
    .trim()
    .max(max, `${field} must be at most ${max} characters`)
    .refine(
      (v) => !v || NAME_RE.test(v),
      `${field} may only contain letters, spaces, hyphens, apostrophes, and periods`,
    )
    .refine(
      (v) => !v || (v.match(/\p{L}/gu)?.length ?? 0) >= 2,
      `${field} must contain at least 2 letters`,
    );
}

function phoneRule() {
  return z
    .string()
    .trim()
    .max(32, "phone must be at most 32 characters")
    .refine(
      (v) => !v || PhoneNumber.parseInput(v) !== undefined,
      "Enter a valid phone number starting with + or country code",
    );
}

function dateOfBirthRule() {
  return z
    .string()
    .trim()
    .refine(
      (v) => !v || /^\d{4}-\d{2}-\d{2}$/.test(v),
      "date_of_birth must be a valid YYYY-MM-DD date",
    )
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
      "date_of_birth must be a real date between 1900 and 2100",
    );
}

function addressRule() {
  return z
    .string()
    .trim()
    .max(256, "residential_address must be at most 256 characters")
    .refine(
      (v) => !v || ![...v].some((c) => c.charCodeAt(0) < 0x20 || c === "" || (c.charCodeAt(0) >= 0x80 && c.charCodeAt(0) <= 0x9F)),
      "residential_address must not contain control characters",
    );
}

function languageRule() {
  return z
    .string()
    .trim()
    .max(16, "language must be at most 16 characters")
    .refine(
      (v) => !v || /^[a-zA-Z]{2,3}([-_][a-zA-Z0-9]{2,8})*$/.test(v),
      "language must be a BCP 47 code such as 'en' or 'en-US'",
    );
}

function currencyRule() {
  return z
    .string()
    .trim()
    .max(3, "base_currency must be a 3-letter code such as 'USD'")
    .refine(
      (v) => !v || /^[a-zA-Z]{3}$/.test(v),
      "base_currency must be a 3-letter code such as 'USD'",
    );
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
    .max(64, "timezone must be at most 64 characters")
    .refine(
      (v) =>
        !v ||
        v === "UTC" ||
        v === "GMT" ||
        IANA.some((area) => v.startsWith(`${area}/`)),
      "timezone must be 'UTC', 'GMT', or an IANA name such as 'Asia/Ho_Chi_Minh'",
    );
}

export const profileEditableSchema = z.object({
  legal_name: nameRule("legal_name", NAME_MAX.legal_name),
  preferred_name: nameRule("preferred_name", NAME_MAX.preferred_name),
  phone: phoneRule(),
  date_of_birth: dateOfBirthRule(),
  nationality: nameRule("nationality", NAME_MAX.nationality),
  tax_residence: nameRule("tax_residence", NAME_MAX.tax_residence),
  residential_address: addressRule(),
  language: languageRule(),
  base_currency: currencyRule(),
  timezone: timezoneRule(),
});

export type ProfileEditable = z.infer<typeof profileEditableSchema>;

/** Validate the form and return a flat `Record<field, error>` — empty = valid. */
export function validateProfileForm(
  form: Record<string, string>,
): Record<string, string> {
  const result = profileEditableSchema.safeParse(form);
  if (result.success) return {};
  const errors: Record<string, string> = {};
  for (const issue of result.error.issues) {
    const field = issue.path[0] as string;
    // Keep the first error per field.
    if (!errors[field]) errors[field] = issue.message;
  }
  return errors;
}
