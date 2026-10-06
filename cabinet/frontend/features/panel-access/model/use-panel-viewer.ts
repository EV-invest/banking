"use client";

import { grantHoldersResource } from "@/entities/grant/model/grant-resource";
import { profileResource } from "@/entities/user/model/profile-resource";
import { useResource } from "@/shared/lib/resource";

export interface PanelViewer {
  /** What the caller may do, from `GET /api/users`. */
  permissions: readonly string[];
  /** The profile has answered. Until then every gate stays closed rather than flashing open. */
  ready: boolean;
}

// A failed profile read still settles: the caller then holds nothing here, instead of a
// skeleton forever.
export function usePanelViewer(): PanelViewer {
  const profile = useResource(profileResource);
  return {
    permissions: profile.data?.permissions ?? [],
    ready: profile.data !== undefined || profile.error !== null,
  };
}

/** Whether the caller manages `namespace`'s grants. Who may is the identity plane's to say
 *  (a seat, or a delegate of the namespace) and no permission tells it, so the roster read
 *  is asked: an answer is a yes, a refusal a no. `null` asks nothing. */
export function usePanelManager(namespace: string | null): boolean {
  return useResource(grantHoldersResource, namespace ?? "").data !== undefined;
}
