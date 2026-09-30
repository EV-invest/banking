// Who may see and change a vertical's panel access — the `allocation:<service_id>` scopes
// the identity plane keeps. Cosmetic, like `shared/lib/roles.ts`: concierge re-checks every
// read and write, so these rules only spare the reader controls that would answer 403.
//
// Two axes that must not be folded together: the caller's GLOBAL role (from the session)
// and their per-scope grants (from `GET /api/users`, `scopes`). The profile deliberately
// leaves the global role out of `scopes`, so each check here reads both.
//
// Kept free of any `@/` import so the node test runner, which resolves no alias, can load it.

/** The roles a scope grant may carry. `viewer` is gone: a read-only holder is an ordinary user. */
export type ScopeRole = "operator" | "admin";

export const SCOPE_ROLES: readonly ScopeRole[] = ["operator", "admin"];

/** Global roles that administer every scope, whatever the caller's own grants say. */
const GLOBAL_SCOPE_ADMINS: readonly string[] = ["admin", "owner"];

/** The shape both `ScopedGrant` and a holder's `grant` share — everything is optional on the wire. */
export interface GrantLike {
  scope?: string;
  role?: string;
}

export function isScopeRole(value: string | undefined): value is ScopeRole {
  return value === "operator" || value === "admin";
}

export function allocationScope(service: string): string {
  return `allocation:${service}`;
}

export function isGlobalScopeAdmin(globalRole: string | undefined): boolean {
  return globalRole !== undefined && GLOBAL_SCOPE_ADMINS.includes(globalRole);
}

/** The caller's role on one allocation's scope, or `null` when they hold none. */
export function scopeRoleOf(scopes: readonly GrantLike[] | undefined, service: string): ScopeRole | null {
  const scope = allocationScope(service);
  const role = scopes?.find((g) => g.scope === scope)?.role;
  return isScopeRole(role) ? role : null;
}

/** May open the vertical's panel at all: any scope role, or a global admin/owner. */
export function canOpenPanel(globalRole: string | undefined, scopes: readonly GrantLike[] | undefined, service: string): boolean {
  return isGlobalScopeAdmin(globalRole) || scopeRoleOf(scopes, service) !== null;
}

/** May list, grant and revoke panel access for this allocation. */
export function canManagePanelAccess(globalRole: string | undefined, scopes: readonly GrantLike[] | undefined, service: string): boolean {
  return isGlobalScopeAdmin(globalRole) || scopeRoleOf(scopes, service) === "admin";
}

/** What the grant form offers. A scope's own admin may seat operators only — minting or
 *  promoting another admin is refused by the plane, so it is not offered. */
export function grantableRoles(globalRole: string | undefined): readonly ScopeRole[] {
  return isGlobalScopeAdmin(globalRole) ? SCOPE_ROLES : ["operator"];
}

/** Whether a holder's row gets a revoke control. A scope admin may drop operators, never an
 *  admin — not even their own grant. */
export function canRevokeHolder(globalRole: string | undefined, holderRole: string | undefined): boolean {
  return isGlobalScopeAdmin(globalRole) || holderRole === "operator";
}

/**
 * The one admin route a scope's own admin may open without a global role —
 * `/admin/allocations/<service>`, the allocation's panel-access page. Zone-relative path.
 */
export function isPanelAccessRoute(pathname: string): boolean {
  return /^\/admin\/allocations\/[^/]+\/?$/.test(pathname);
}
