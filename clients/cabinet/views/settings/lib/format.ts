// Name/initials helpers for the settings surface, derived from the session email
// (the only real identity the BFF exposes today). Mirrors the sidebar AccountChip /
// profile derivation so the name reads consistently across the shell.

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
