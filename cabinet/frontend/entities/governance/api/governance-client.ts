"use client";

// The owners' room, over the ordinary signed-in transport. Every call here is gated by the
// session the rest of the cabinet uses; the token-and-code surface is the separate,
// deliberately session-free `entities/approval` client.

import type {
  AdmissionVote,
  Consilium,
  ConsiliumList,
  OwnerAdmission,
  OwnerAdmissionList,
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

export function fetchAdmissions(): Promise<OwnerAdmissionList> {
  return getJson<OwnerAdmissionList>("/api/owners/admissions");
}

export function proposeAdmission(candidateUserId: string, reason: string): Promise<OwnerAdmission> {
  return postJson<OwnerAdmission>("/api/owners/admissions", { candidate_user_id: candidateUserId, reason });
}

/**
 * Vote on an admission.
 *
 * The body word is `admit` or `reject` — the ownership plane's admission vocabulary, which
 * the BFF parses with its own `AdmissionVote::parse` and which rejects `remove`/`keep`
 * outright. The narrow parameter type is the point: this endpoint and
 * {@link voteOnRemoval} take structurally identical arguments, so nothing but the types
 * stops a call site sending one plane's verb to the other's route.
 */
export function voteOnAdmission(admissionId: string, vote: AdmissionVote): Promise<OwnerAdmission> {
  return postJson<OwnerAdmission>(`/api/owners/admissions/${encodeURIComponent(admissionId)}/vote`, { vote });
}

export function cancelAdmission(admissionId: string): Promise<OwnerAdmission> {
  return postJson<OwnerAdmission>(`/api/owners/admissions/${encodeURIComponent(admissionId)}/cancel`, {});
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
