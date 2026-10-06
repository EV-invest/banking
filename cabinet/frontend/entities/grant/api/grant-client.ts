// Browser → BFF tenant-grant client. The BFF gates on the session alone and forwards to the
// identity plane, which decides who may read or change a namespace; transport, CSRF and
// session handling belong to `@/shared/lib/api-client`.

import type { GrantHolderList, PermissionGrant } from "@/shared/contracts";
import { getJson, requestJson } from "@/shared/lib/api-client";

const grantsPath = (namespace: string): `/${string}` => `/api/admin/tenants/${encodeURIComponent(namespace)}/grants`;

export const fetchGrantHolders = (namespace: string): Promise<GrantHolderList> => getJson(grantsPath(namespace));

// By email, never by id: a delegate is refused the id form (it would turn the roster's
// `granted_by` into an address lookup), and email is what the form collects.
export const grantPermission = (namespace: string, email: string, target: string): Promise<PermissionGrant> =>
  requestJson(grantsPath(namespace), { method: "POST", body: { email, target } });

// By the id the roster handed back — both forms are accepted for a revocation.
export const revokePermission = (namespace: string, userId: string, target: string): Promise<{ ok: boolean }> =>
  requestJson(grantsPath(namespace), { method: "DELETE", body: { user_id: userId, target } });
