"use client";

import { fetchGrantHolders } from "@/entities/grant/api/grant-client";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource } from "@/shared/lib/resource";

// Operational window, like the money grants beside it: whoever opens the roster is about
// to change it and is looking at it now. It is also how the console learns whether the
// caller manages the namespace at all — a 403 here is the "no".
export const grantHoldersResource = defineResource({
  name: "grant.holders",
  fetch: fetchGrantHolders,
  key: (namespace) => namespace,
  revalidate: 10,
  tags: [TAG.grantHolders],
  enabled: (namespace) => namespace.trim().length > 0,
});
