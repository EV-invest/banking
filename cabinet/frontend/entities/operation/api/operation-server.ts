import "server-only";

import { bffRead, type ServerRead } from "@/shared/api/server/bff";
import type { OperationList } from "@/shared/contracts";

// The activity timeline for server components — the same endpoint `operation-client.ts`
// reads from the browser, so its answer seeds `operationsResource` under the same key.

/** Refuses only what would break the screens: a non-object, or `operations` that is not a list. */
function isOperationList(body: unknown): body is OperationList {
  if (typeof body !== "object" || body === null) return false;
  const { operations } = body as Record<string, unknown>;
  return operations === undefined || Array.isArray(operations);
}

/** `limit` as `fetchOperations` takes it: omitted for the full list Operations and Profile show. */
export function readOperations(limit?: number): Promise<ServerRead<OperationList> | null> {
  return bffRead(limit ? `/api/operations?limit=${limit}` : "/api/operations", isOperationList);
}
