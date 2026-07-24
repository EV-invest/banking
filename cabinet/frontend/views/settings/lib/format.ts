// Name/initials helpers for the settings surface, derived from the session email
// (the only real identity the BFF exposes today). Mirrors the sidebar AccountChip /
// profile derivation so the name reads consistently across the shell.

/** Max chars a display name takes up before ellipsis in the chip and headings. */
const MAX_DISPLAY = 32;

/** Truncate a profile/real name for display so an overlong value doesn't break
 *  the header or chip layout. Surrogate-pair-safe. */
export function truncateName(name: string, max: number = MAX_DISPLAY): string {
  if (name.length <= max) return name;
  // Walk grapheme clusters conservatively — slice at char boundary just inside max,
  // then replace the last 3 chars with "…".
  return `${name.slice(0, max - 1)}…`;
}

/** A human display name from an email handle: "ada.lovelace@x" → "Ada L." */
export function displayName(email: string | null | undefined): string {
  if (email === undefined) return "…";
  if (!email) return "Account";
  const handle = email.split("@")[0] ?? email;
  const parts = handle.split(/[._-]+/).filter(Boolean);
  const first = parts[0] ? cap(parts[0]) : handle;
  const last = parts[1] ? `${parts[1][0]!.toUpperCase()}.` : "";
  return [first, last].filter(Boolean).join(" ");
}

/** Up-to-two-letter initials: "ada.lovelace@x" → "AL". */
export function initialsOf(email: string | null | undefined): string {
  if (!email) return "EV";
  const parts = (email.split("@")[0] ?? "").split(/[._-]+/).filter(Boolean);
  const a = parts[0]?.[0] ?? email[0] ?? "E";
  const b = parts[1]?.[0] ?? "";
  return (a + b).toUpperCase();
}

function cap(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
