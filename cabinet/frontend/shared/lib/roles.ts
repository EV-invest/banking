// Who a role-gated surface is shown to. Cosmetic only: every surface this gates is also
// authorized server-side, so hiding it spares the reader a screen of 403s rather than
// keeping anything from them.
//
// Roles are the platform's wire vocabulary as `/api/auth/session` returns it —
// `investor` / `operator` / `admin` / `owner` (concierge `domain/src/authz.rs`).
//
// Kept free of any `@/` import so the node test runner, which resolves no alias, can load it.

/**
 * `roles` undefined means ungated: shown to everyone. A gated surface needs a known role
 * — while the session is still loading (`role` undefined) it stays hidden rather than
 * flashing in and out as the principal resolves.
 */
export function visibleFor(roles: readonly string[] | undefined, role: string | undefined): boolean {
  if (roles === undefined) return true;
  return role !== undefined && roles.includes(role);
}
