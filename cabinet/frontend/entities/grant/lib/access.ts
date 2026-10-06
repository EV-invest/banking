// Who may see a vertical's panel and its access page — read off what the identity plane
// says the caller holds. Cosmetic, like `shared/lib/roles.ts`: concierge re-checks every
// read and write, so these rules only spare the reader controls that would answer 403.
//
// Kept free of any `@/` import so the node test runner, which resolves no alias, can load it.

/** The tenant namespace whose panel serves an allocation. An allocation without one has no panel. */
export const TENANTS = { service_arb: "sa" } as const;

export function tenantOf(service: string): string | null {
  return Object.hasOwn(TENANTS, service) ? TENANTS[service as keyof typeof TENANTS] : null;
}

/** An allocation id as the registry keys it — `[a-z0-9_]{1,64}`. A route carrying anything
 *  else is a 404, not a request. */
export function isServiceId(value: string): boolean {
  return /^[a-z0-9_]{1,64}$/.test(value);
}

/** May open the tenant's panel at all: holds anything in its namespace. `GetMe.permissions`
 *  is concrete, and a seat that holds every tenant's permissions holds this one's too. */
export function canOpenPanel(permissions: readonly string[], namespace: string): boolean {
  return permissions.some((p) => p.startsWith(`${namespace}:`));
}

/**
 * The one admin route a tenant's delegate may open without a console role —
 * `/admin/allocations/<service>`, the allocation's panel-access page. Zone-relative path.
 */
export function isPanelAccessRoute(pathname: string): boolean {
  return /^\/admin\/allocations\/[a-z0-9_]{1,64}\/?$/.test(pathname);
}
