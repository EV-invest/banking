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
    // Grant: an address no account holds (404) and one the plane will not seat — disabled,
    // or shared by two accounts (412) — read the same. Telling them apart would make the
    // form a probe for which addresses have cabinet accounts.
    case 404:
      return action === "revoke" ? "panelAccess.err.notHolder" : "panelAccess.err.cannotGrant";
    case 412:
      return action === "revoke" ? null : "panelAccess.err.cannotGrant";
    // RESOURCE_EXHAUSTED, which the BFF maps to 429: the plane's per-caller limit.
    case 429:
      return "panelAccess.err.tooMany";
    default:
      return null;
  }
}
