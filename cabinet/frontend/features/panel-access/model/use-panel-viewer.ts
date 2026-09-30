"use client";

import { profileResource } from "@/entities/user/model/profile-resource";
import type { ScopedGrant } from "@/shared/contracts";
import { useResource } from "@/shared/lib/resource";
import { useSession } from "@/shared/lib/use-session";

export interface PanelViewer {
  /** The caller's global role, from the session. */
  role: string | undefined;
  /** The caller's active scope grants, from `GET /api/users`. */
  scopes: readonly ScopedGrant[];
  /** Both reads have answered. Until then every gate stays closed rather than flashing open. */
  ready: boolean;
}

// The two halves every panel-access gate reads. A failed profile read still settles: the
// caller then keeps what the global role alone gives them, instead of a skeleton forever.
export function usePanelViewer(): PanelViewer {
  const session = useSession();
  const profile = useResource(profileResource);
  return {
    role: session?.user?.role,
    scopes: profile.data?.scopes ?? [],
    ready: session !== null && (profile.data !== undefined || profile.error !== null),
  };
}
