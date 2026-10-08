// Stands in for `@/shared/lib/resource`, `@/entities/admin/model/admin-resource` and
// `@/shared/ui/resource-error` (whose extensionless re-export type stripping cannot load) in
// `user-picker.test.ts`: the whole-directory read, answered by the test.
//
// The real resource layer reads through `shared/lib/api-client.ts`, which Node's type
// stripping cannot load, and under `react-dom/server` it never leaves its cold state anyway.
// What the picker renders from is the snapshot alone, so the test sets that snapshot.

import type { AdminUserSummary } from "@/shared/contracts/admin";
import type { ResourceSnapshot } from "@/shared/lib/resource";

export const userDirectoryResource = { name: "admin.userDirectory" };

let snapshot: ResourceSnapshot<AdminUserSummary[]> = loading();

/** The directory has not answered yet. */
export function loading(): ResourceSnapshot<AdminUserSummary[]> {
  return { data: undefined, error: null, isLoading: true, isValidating: false, refresh: async () => undefined };
}

/** The directory answered with these people. */
export function answered(users: AdminUserSummary[]): ResourceSnapshot<AdminUserSummary[]> {
  return { data: users, error: null, isLoading: false, isValidating: false, refresh: async () => undefined };
}

export function directoryIs(next: ResourceSnapshot<AdminUserSummary[]>): void {
  snapshot = next;
}

/** An investor as the directory lists one; only the id and the email matter to the picker. */
export function investor(user_id: string, email: string): AdminUserSummary {
  return { user_id, email, status: "active", kyc_level: 1, role: "investor", role_is_break_glass: false, token_version: "1", created_at: "1780000000", suspended_by: "", hold_expires_at: "0" };
}

export function useResource(): ResourceSnapshot<AdminUserSummary[]> {
  return snapshot;
}

export function ResourceError(): null {
  return null;
}
