// Display normalisation for the two contact fields, via the `Email` and `PhoneNumber`
// TypeObjects — so a stored value reads the same in the mobile rows, the desktop form and
// the Security card.

import { Email, PhoneNumber } from "@evinvest/types";

/** Normalise the stored email for display via the `Email` TypeObject. */
export function formatEmail(raw?: string | null): string {
  if (!raw) return "";
  const e = Email.parseInput(raw) ?? Email.fromUnsafe(raw);
  return Email.raw(e);
}

export function formatPhone(raw: string): string {
  if (!raw) return "";
  const pn = PhoneNumber.parseInput(raw) ?? PhoneNumber.fromUnsafe(raw);
  return PhoneNumber.format(pn);
}
