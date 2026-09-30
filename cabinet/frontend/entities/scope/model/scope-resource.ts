"use client";

import { fetchScopeHolders } from "@/entities/scope/api/scope-client";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource } from "@/shared/lib/resource";

// Operational window, like the money grants beside it: whoever opens the roster is about
// to change it and is looking at it now.
export const scopeHoldersResource = defineResource({
  name: "scope.holders",
  fetch: fetchScopeHolders,
  key: (service) => service,
  revalidate: 10,
  tags: [TAG.scopeHolders],
  enabled: (service) => service.trim().length > 0,
});
