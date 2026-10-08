import "server-only";

import { bffRead, type ServerRead } from "@/shared/api/server/bff";
import { hasOptionalLists, isJsonObject } from "@/shared/api/server/shape";
import type { AllocationList, FeePolicy, FundNav, PositionList, RedemptionList } from "@/shared/contracts";

// The fund reads for server components — the same paths `fund-client.ts` asks from the
// browser, so each answer can seed its resource directly. A blank service is no read at
// all, as there: the BFF would only answer it with a 400.

function isAllocationList(body: unknown): body is AllocationList {
  // The BFF writes this list by hand and always includes it; a body without it is not one.
  return isJsonObject(body) && Array.isArray(body.allocations);
}

function isPositionList(body: unknown): body is PositionList {
  return hasOptionalLists(body, ["positions"]);
}

function isRedemptionList(body: unknown): body is RedemptionList {
  return hasOptionalLists(body, ["redemptions"]);
}

// Scalar-only DTOs: nothing on them is indexed or mapped, so an object is the whole check.
const isFundNav = (body: unknown): body is FundNav => isJsonObject(body);
const isFeePolicy = (body: unknown): body is FeePolicy => isJsonObject(body);

const query = (service: string) => `service=${encodeURIComponent(service)}`;
const named = (service: string) => service.trim().length > 0;

export function readAllocations(): Promise<ServerRead<AllocationList> | null> {
  return bffRead("/api/allocations", isAllocationList);
}

export function readPositions(): Promise<ServerRead<PositionList> | null> {
  return bffRead("/api/funds/positions", isPositionList);
}

export function readRedemptions(): Promise<ServerRead<RedemptionList> | null> {
  return bffRead("/api/funds/redemptions", isRedemptionList);
}

export async function readFundNav(service: string): Promise<ServerRead<FundNav> | null> {
  return named(service) ? bffRead(`/api/funds/nav?${query(service)}`, isFundNav) : null;
}

export async function readFeePolicy(service: string): Promise<ServerRead<FeePolicy> | null> {
  return named(service) ? bffRead(`/api/funds/fee-policy?${query(service)}`, isFeePolicy) : null;
}
