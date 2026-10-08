import "server-only";

import { bffRead, type ServerRead } from "@/shared/api/server/bff";
import { hasOptionalLists } from "@/shared/api/server/shape";
import type { OperationList } from "@/shared/contracts";

// The timeline read for server components — the path `operation-client.ts` asks from the
// browser, so its answer seeds `operationsResource` under the same key.

function isOperationList(body: unknown): body is OperationList {
  return hasOptionalLists(body, ["operations"]);
}

/** `limit` as `fetchOperations` takes it: Home's preview passes one; Operations and Profile read the full list. */
export function readOperations(limit?: number): Promise<ServerRead<OperationList> | null> {
  return bffRead(limit ? `/api/operations?limit=${limit}` : "/api/operations", isOperationList);
}
