import "server-only";

import { bffRead, type ServerRead } from "@/shared/api/server/bff";
import { hasOptionalLists, isJsonObject } from "@/shared/api/server/shape";
import type { AllocationList, PositionList } from "@/shared/contracts";

// The fund reads for server components — the same paths `fund-client.ts` asks from the
// browser, so each answer can seed its resource directly.

function isAllocationList(body: unknown): body is AllocationList {
  // The BFF writes this list by hand and always includes it; a body without it is not one.
  return isJsonObject(body) && Array.isArray(body.allocations);
}

function isPositionList(body: unknown): body is PositionList {
  return hasOptionalLists(body, ["positions"]);
}

export function readAllocations(): Promise<ServerRead<AllocationList> | null> {
  return bffRead("/api/allocations", isAllocationList);
}

export function readPositions(): Promise<ServerRead<PositionList> | null> {
  return bffRead("/api/funds/positions", isPositionList);
}
