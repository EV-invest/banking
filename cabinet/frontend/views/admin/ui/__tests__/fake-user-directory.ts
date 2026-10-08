// Stands in for both `@/shared/lib/resource` and `@/entities/admin/model/admin-resource` in
// `user-picker.test.ts`: the `/api/admin/users` read, answered by the test.
//
// The real resource layer reads through `shared/lib/api-client.ts`, which Node's type
// stripping cannot load, and under `react-dom/server` it never leaves its cold state anyway.
// What the picker renders from is the snapshot alone, so the test sets that snapshot.

import type { AdminUserList, AdminUserSummary } from "@/shared/contracts/admin";
import type { ResourceSnapshot } from "@/shared/lib/resource";

export const usersResource = { name: "admin.users" };

let snapshot: ResourceSnapshot<AdminUserList> = loading();

/** The directory has not answered yet. */
export function loading(): ResourceSnapshot<AdminUserList> {
  return { data: undefined, error: null, isLoading: true, isValidating: false, refresh: async () => undefined };
}

/** The directory answered with these people. */
export function answered(users: AdminUserSummary[]): ResourceSnapshot<AdminUserList> {
  return { data: { users, total: String(users.length) }, error: null, isLoading: false, isValidating: false, refresh: async () => undefined };
}

export function directoryIs(next: ResourceSnapshot<AdminUserList>): void {
  snapshot = next;
}

/** An investor as the directory lists one; only the id and the email matter to the picker. */
export function investor(user_id: string, email: string): AdminUserSummary {
  return { user_id, email, status: "active", kyc_level: 1, role: "investor", role_is_break_glass: false, token_version: "1", created_at: "1780000000", suspended_by: "", hold_expires_at: "0" };
}

export function useResource(): ResourceSnapshot<AdminUserList> {
  return snapshot;
}
