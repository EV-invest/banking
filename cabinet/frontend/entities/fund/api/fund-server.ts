import "server-only";

import { bffRead, type ServerRead } from "@/shared/api/server/bff";
import type { AllocationList, PositionList } from "@/shared/contracts";

// Fund reads for server components — the same endpoints `fund-client.ts` reads from the
// browser, so each answer seeds its resource directly. Every proto3 JSON field is
// optional; the guards refuse only a body the screens would break on.

function hasListOrNone(body: unknown, field: string): boolean {
  if (typeof body !== "object" || body === null) return false;
  const value = (body as Record<string, unknown>)[field];
  return value === undefined || Array.isArray(value);
}

const isPositionList = (body: unknown): body is PositionList => hasListOrNone(body, "positions");
const isAllocationList = (body: unknown): body is AllocationList => hasListOrNone(body, "allocations");

export function readPositions(): Promise<ServerRead<PositionList> | null> {
  return bffRead("/api/funds/positions", isPositionList);
}

/** The investor catalog — on the server only to turn fund slugs into names on first paint. */
export function readAllocations(): Promise<ServerRead<AllocationList> | null> {
  return bffRead("/api/allocations", isAllocationList);
}
