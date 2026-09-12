"use client";

// The four governance reads, and the mutations that move them.
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
  cancelAdmission as cancelAdmissionRequest,
  cancelConsilium as cancelConsiliumRequest,
  cancelRemoval as cancelRemovalRequest,
  cancelUserProposal as cancelUserProposalRequest,
  fetchAdmissions,
  fetchConsilium,
  fetchOwners,
  fetchRemovals,
  fetchUserProposals,
  openAdminAdmission as openAdminAdmissionRequest,
  openRevenuePayout as openRevenuePayoutRequest,
  openUserReinstatement as openUserReinstatementRequest,
  openUserSuspension as openUserSuspensionRequest,
  proposeAdmission as proposeAdmissionRequest,
  proposeRemoval as proposeRemovalRequest,
  resignOwnership as resignOwnershipRequest,
  voteOnAdmission as voteOnAdmissionRequest,
  voteOnRemoval as voteOnRemovalRequest,
  voteOnUserProposal as voteOnUserProposalRequest,
} from "@/entities/governance/api/governance-client";
import type { AdmissionVote, Consilium, OwnerAdmission, OwnerRemoval, ProposalVote, RemovalVote, UserProposal } from "@/shared/contracts/governance";
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

export const admissionsResource = defineResource({
  name: "governance.admissions",
  fetch: fetchAdmissions,
  revalidate: 15,
  tags: [TAG.admissions],
});

export const userProposalsResource = defineResource({
  name: "governance.userProposals",
  fetch: () => fetchUserProposals(),
  revalidate: 15,
  tags: [TAG.userProposals],
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
 * room moved", never what — so the only honest response is to re-read all four. They are
 * four small reads against one plane, and guessing which one changed would be a way to
 * miss the one that did. Admissions joined this list rather than getting a channel of
 * their own for exactly that reason.
 */
export const GOVERNANCE_TAGS = [TAG.owners, TAG.removals, TAG.admissions, TAG.userProposals, TAG.consilium] as const;

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

/** Open an admission. Changes the admission list; no seat moves until it carries. */
export async function proposeAdmission(candidateUserId: string, reason: string): Promise<OwnerAdmission> {
  const admission = await proposeAdmissionRequest(candidateUserId, reason);
  revalidateTag(TAG.admissions);
  return admission;
}

/**
 * Vote on an admission.
 *
 * Names every governance tag for the mirror image of the reason a removal vote does: an
 * `admit` can be the one that carries, and an admission that carries seats a new owner —
 * which grows the roster, can lift the fund back over the payout floor, and changes the
 * denominator every open removal is counted against. As everywhere else here, the response
 * is not written into the cache: the tally is the server's, computed under a row lock.
 */
export async function voteOnAdmission(admissionId: string, vote: AdmissionVote): Promise<OwnerAdmission> {
  const admission = await voteOnAdmissionRequest(admissionId, vote);
  revalidateTag(...GOVERNANCE_TAGS);
  return admission;
}

export async function cancelAdmission(admissionId: string): Promise<OwnerAdmission> {
  const admission = await cancelAdmissionRequest(admissionId);
  revalidateTag(TAG.admissions);
  return admission;
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

// ── user proposals ────────────────────────────────────────────────────────────
//
// Opened from the operator console's user drawer and voted on in the owners' room, so both
// tags are named on every write: the console's user list shows the standing a suspension
// changes and the role an admin admission grants, and the room shows the proposal itself.
// Routed through here rather than called straight from the client for exactly that reason —
// a raw call cannot say what it moved.

/** Ask the owners to make a hold permanent. Nothing about the user moves until it carries. */
export async function openUserSuspension(userId: string, reason: string): Promise<UserProposal> {
  const proposal = await openUserSuspensionRequest(userId, reason);
  revalidateTag(TAG.userProposals);
  return proposal;
}

/** Ask the owners to undo their own verdict. */
export async function openUserReinstatement(userId: string, reason: string): Promise<UserProposal> {
  const proposal = await openUserReinstatementRequest(userId, reason);
  revalidateTag(TAG.userProposals);
  return proposal;
}

/** Ask the owners to grant `Role::Admin`. Taking it away is not here — that is one call. */
export async function openAdminAdmission(userId: string, reason: string): Promise<UserProposal> {
  const proposal = await openAdminAdmissionRequest(userId, reason);
  revalidateTag(TAG.userProposals);
  return proposal;
}

/**
 * Vote on a user proposal.
 *
 * Names the governance tags AND the console's user list, because a vote can be the one that
 * carries and every kind changes something the console renders: a suspension blocks the
 * account, a reinstatement unblocks it, an admin admission changes the role. As everywhere
 * else here the response is not written into the cache — the tally is the server's, counted
 * under a row lock, and a threshold frozen at open is not something a client may re-derive.
 */
export async function voteOnUserProposal(proposalId: string, vote: ProposalVote): Promise<UserProposal> {
  const proposal = await voteOnUserProposalRequest(proposalId, vote);
  revalidateTag(...GOVERNANCE_TAGS, TAG.adminUsers);
  return proposal;
}

export async function cancelUserProposal(proposalId: string): Promise<UserProposal> {
  const proposal = await cancelUserProposalRequest(proposalId);
  revalidateTag(TAG.userProposals);
  return proposal;
}
