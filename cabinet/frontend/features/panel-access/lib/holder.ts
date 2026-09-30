// How a holder row names people. Kept free of any `@/` import so the node test runner can load it.

export interface HolderLike {
  grant?: { user_id?: string };
  email?: string;
  legal_name?: string;
  preferred_name?: string;
}

/** The name to show beside the email: what the person asked to be called, else their legal
 *  name (empty for a caller who is only the scope's admin), else nothing. */
export function holderName(holder: HolderLike): string {
  return holder.preferred_name?.trim() || holder.legal_name?.trim() || "";
}

/** `granted_by` is a user id. Resolve it against the roster when the granter holds the scope
 *  too; otherwise show the id — the scope's admin is not entitled to look anyone else up. */
export function granterLabel(grantedBy: string | undefined, holders: readonly HolderLike[]): string {
  if (!grantedBy) return "—";
  return holders.find((h) => h.grant?.user_id === grantedBy)?.email || grantedBy;
}
