"use client";

// The three governance reads, and the mutations that move them.
//
// Every mutation here invalidates rather than publishing its own response, which is the
// opposite of what `saveProfile` does — and the difference is the point. A profile PATCH
// answers with the whole profile, and the caller is the only author of it. A vote answers
// with one removal, but what the reader is looking at is a *tally*, and the authority on a
// tally is the server that counted it under a row lock (docs/CONSILIUM.md, policy 14).
// Writing a mutation's own view of the count into the cache would put a number on screen
// that was assembled by the client — which is the exact failure mode the whole design of
// this feature is arranged to prevent. So: name the tags, let the cache re-read.
//
// The revalidate windows are short. This is a room several people are acting in at once,
// and the socket (`./consilium-socket`) is what actually keeps it current; these windows
// are the floor under a socket that is down, not the mechanism.

import {
  cancelConsilium as cancelConsiliumRequest,
  cancelRemoval as cancelRemovalRequest,
  fetchConsilium,
  fetchOwners,
  fetchRemovals,
  openRevenuePayout as openRevenuePayoutRequest,
  proposeRemoval as proposeRemovalRequest,
  resignOwnership as resignOwnershipRequest,
  voteOnRemoval as voteOnRemovalRequest,
} from "@/entities/governance/api/governance-client";
import type { Consilium, OwnerRemoval, RemovalVote } from "@/shared/contracts/governance";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource, revalidateTag } from "@/shared/lib/resource";

export const ownersResource = defineResource({
  name: "governance.owners",
  fetch: fetchOwners,
  revalidate: 30,
  tags: [TAG.owners],
});

export const removalsResource = defineResource({
  name: "governance.removals",
  fetch: fetchRemovals,
  revalidate: 15,
  tags: [TAG.removals],
});

export const consiliumResource = defineResource({
  name: "governance.consilium",
  fetch: fetchConsilium,
  revalidate: 15,
  tags: [TAG.consilium],
});

/**
 * Every tag this feature owns.
 *
 * The socket carries a revision, not a subject — a frame says "something in the owners'
 * room moved", never what — so the only honest response is to re-read all three. They are
 * three small reads against one plane, and guessing which one changed would be a way to
 * miss the one that did.
 */
export const GOVERNANCE_TAGS = [TAG.owners, TAG.removals, TAG.consilium] as const;

/** Re-read the authoritative snapshot of the whole room. */
export function refreshGovernance(): void {
  revalidateTag(...GOVERNANCE_TAGS);
}

/** Open a removal. Changes the removal list; the roster does not move until it carries. */
export async function proposeRemoval(targetUserId: string, reason: string): Promise<OwnerRemoval> {
  const removal = await proposeRemovalRequest(targetUserId, reason);
  revalidateTag(TAG.removals);
  return removal;
}

/**
 * Vote on a removal.
 *
 * Also names the roster and the consilium: a vote can be the one that carries, and a
 * removal that carries both shrinks the roster and can push the fund below the payout
 * floor — which changes what the payout card is allowed to offer.
 */
export async function voteOnRemoval(removalId: string, vote: RemovalVote): Promise<OwnerRemoval> {
  const removal = await voteOnRemovalRequest(removalId, vote);
  revalidateTag(...GOVERNANCE_TAGS);
  return removal;
}

export async function cancelRemoval(removalId: string): Promise<OwnerRemoval> {
  const removal = await cancelRemovalRequest(removalId);
  revalidateTag(TAG.removals);
  return removal;
}

/** Give up your own seat — the roster shrinks, and with it what a payout can clear. */
export async function resignOwnership(confirmEmail: string): Promise<void> {
  await resignOwnershipRequest(confirmEmail);
  revalidateTag(...GOVERNANCE_TAGS);
}

export async function openRevenuePayout(body: { network: string; address: string; amount: string; memo?: string }): Promise<Consilium> {
  const consilium = await openRevenuePayoutRequest(body);
  revalidateTag(TAG.consilium);
  return consilium;
}

export async function cancelConsilium(consiliumId: string): Promise<Consilium> {
  const consilium = await cancelConsiliumRequest(consiliumId);
  revalidateTag(TAG.consilium);
  return consilium;
}
