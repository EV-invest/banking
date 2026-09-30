// How a holder row names people. Kept free of any `@/` import so the node test runner can load it.

export interface HolderLike {
  email?: string;
  legal_name?: string;
  preferred_name?: string;
}

/** The name to show beside the email: what the person asked to be called, else their legal
 *  name (empty for a caller who is only the scope's admin), else nothing. */
export function holderName(holder: HolderLike): string {
  return holder.preferred_name?.trim() || holder.legal_name?.trim() || "";
}
