"use client";

// The owners' room, over the ordinary signed-in transport. Every call here is gated by the
// session the rest of the cabinet uses; the token-and-code surface is the separate,
// deliberately session-free `entities/approval` client.

import type {
  Consilium,
  ConsiliumList,
  OwnerList,
  OwnerRemoval,
  OwnerRemovalList,
  RemovalVote,
} from "@/shared/contracts/governance";
import { getJson, postJson } from "@/shared/lib/api-client";

export function fetchOwners(): Promise<OwnerList> {
  return getJson<OwnerList>("/api/owners");
}

export function fetchRemovals(): Promise<OwnerRemovalList> {
  return getJson<OwnerRemovalList>("/api/owners/removals");
}

export function fetchConsilium(): Promise<ConsiliumList> {
  return getJson<ConsiliumList>("/api/consilium");
}

export function proposeRemoval(targetUserId: string, reason: string): Promise<OwnerRemoval> {
  return postJson<OwnerRemoval>("/api/owners/removals", { target_user_id: targetUserId, reason });
}

export function voteOnRemoval(removalId: string, vote: RemovalVote): Promise<OwnerRemoval> {
  return postJson<OwnerRemoval>(`/api/owners/removals/${encodeURIComponent(removalId)}/vote`, { vote });
}

export function cancelRemoval(removalId: string): Promise<OwnerRemoval> {
  return postJson<OwnerRemoval>(`/api/owners/removals/${encodeURIComponent(removalId)}/cancel`, {});
}

/**
 * Give up your own seat. No consilium — resigning is not something the other owners get to
 * veto — but the same floor applies, and the BFF refuses when the fund would drop below it.
 *
 * `confirm_email` is the typed-out confirmation, not an identifier: the server already
 * knows who is asking. It exists so this cannot be a mis-click.
 */
export function resignOwnership(confirmEmail: string): Promise<void> {
  return postJson<void>("/api/owners/resign", { confirm_email: confirmEmail });
}

export function openRevenuePayout(body: { network: string; address: string; amount: string; memo?: string }): Promise<Consilium> {
  return postJson<Consilium>("/api/consilium/revenue-payout", body);
}

export function cancelConsilium(consiliumId: string): Promise<Consilium> {
  return postJson<Consilium>(`/api/consilium/${encodeURIComponent(consiliumId)}/cancel`, {});
}
