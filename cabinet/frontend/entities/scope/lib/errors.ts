// What a refused scope call means to the person who made it. The BFF relays concierge's
// verdict as `{ error }` prose in English; the status is the part that is ours to word.
//
// Kept free of any `@/` import so the node test runner, which resolves no alias, can load it.

export type ScopeAction = "list" | "grant" | "revoke";

/**
 * A catalogue key for the status, or `null` to fall back to the generic error surface
 * (401 is healed or turned into a sign-in by the api client before it reaches here, and a
 * 5xx is not specific to scopes).
 */
export function scopeErrorKey(status: number, action: ScopeAction): string | null {
  switch (status) {
    case 400:
      return "panelAccess.err.invalid";
    case 403:
      return action === "list" ? "panelAccess.err.forbiddenList" : "panelAccess.err.forbidden";
    // Grant: no account holds that address. Revoke: the person holds no active grant (a
    // stranger's id answers the same, by design — the plane leaks no membership).
    case 404:
      return action === "revoke" ? "panelAccess.err.notHolder" : "panelAccess.err.noAccount";
    // FAILED_PRECONDITION: the account is disabled, or its email names more than one account.
    case 412:
      return "panelAccess.err.unavailable";
    default:
      return null;
  }
}
