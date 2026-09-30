// Browser → BFF panel-access client. The BFF gates on the session alone and forwards to
// the identity plane, which decides who may read or change a scope; transport, CSRF and
// session handling belong to `@/shared/lib/api-client`.

import type { ScopedGrant, ScopeHolderList } from "@/shared/contracts";
import { getJson, requestJson } from "@/shared/lib/api-client";
import type { ScopeRole } from "@/entities/scope/lib/access";

const scopesPath = (service: string): `/${string}` => `/api/admin/allocations/${encodeURIComponent(service)}/scopes`;

export const fetchScopeHolders = (service: string): Promise<ScopeHolderList> => getJson(scopesPath(service));

// By email, never by id: a scope's own admin is refused the id form (it would turn the
// roster's `granted_by` into an address lookup), and email is what the form collects.
export const grantScope = (service: string, email: string, role: ScopeRole): Promise<ScopedGrant> =>
  requestJson(scopesPath(service), { method: "POST", body: { email, role } });

// By the id the roster handed back — both forms are accepted for a revocation.
export const revokeScope = (service: string, userId: string): Promise<{ ok: boolean }> =>
  requestJson(scopesPath(service), { method: "DELETE", body: { user_id: userId } });
