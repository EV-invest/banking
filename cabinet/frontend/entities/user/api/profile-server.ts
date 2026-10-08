import "server-only";

import { bffRead, type ServerRead } from "@/shared/api/server/bff";
import type { UserProfile } from "@/shared/contracts";

// The caller's profile for server components — the endpoint `profile-client.ts` reads, so
// the answer seeds `profileResource` and with it the account chip and Settings.

/** Every field is optional and read as a scalar; only a non-object is refused. */
function isUserProfile(body: unknown): body is UserProfile {
  return typeof body === "object" && body !== null && !Array.isArray(body);
}

export function readProfile(): Promise<ServerRead<UserProfile> | null> {
  return bffRead("/api/users", isUserProfile);
}
