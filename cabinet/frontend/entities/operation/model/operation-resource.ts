"use client";

// The activity timeline, cached per requested length. Home asks the hub for the six most
// recent (so the six shown are the six most recent across all kinds, not the newest six of
// whatever the page happened to hold) and Operations asks for the full list — two different
// reads, so two cache keys.
//
// No mutation lives here: nothing writes an operation directly. The timeline moves because
// a withdrawal, subscription or redemption moved it, and each of those names `operations`
// in its own invalidation (see `shared/lib/cache-tags.ts`).

import { fetchOperations } from "@/entities/operation/api/operation-client";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource } from "@/shared/lib/resource";

/** How many operations Home's preview card asks the hub for — the cache key its warm-up uses. */
export const RECENT_OPS = 6;

export const operationsResource = defineResource({
  name: "operations",
  fetch: fetchOperations,
  key: (limit) => String(limit ?? "all"),
  tags: [TAG.operations],
});
