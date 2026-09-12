// The form's draft of an order, and the two facts it previews before the confirm click.
//
// `tier` and `requirement` are the money plane's to decide (`shared/contracts/payments.ts`),
// and the open request carries neither. They are worked out here anyway so the operator
// is not surprised by WHO gets emailed — every owner, or one investor — and the plane's
// answer, echoed back on the opened order, is the one the list renders.

// Kept free of path-alias VALUE imports: `terms.test.ts` runs under Node's own resolver,
// which knows nothing of `@/`. The words for a draft live next door in `words.ts`.

import type { OpenPaymentRequest, Party, PartyKind, PaymentRequirement, PaymentTier } from "@/shared/contracts/payments";

export const PARTY_KINDS: readonly PartyKind[] = ["piggybank", "revenue", "service", "user"];

/** What the destination picker offers: every internal party, plus an address. */
export type EndKind = PartyKind | "external";

export const END_KINDS: readonly EndKind[] = [...PARTY_KINDS, "external"];

/**
 * One end as the form holds it. Every field is kept whatever the kind, so switching the
 * kind back and forth does not lose a typed address or a picked product.
 */
export interface EndDraft {
  kind: EndKind;
  /** The product slug or the concierge user id; ignored for the singletons and addresses. */
  id: string;
  /** What the picker showed for `id` — a product's title, a user's email. Display only. */
  name: string;
  network: string;
  address: string;
}

export const EMPTY_END: EndDraft = { kind: "piggybank", id: "", name: "", network: "", address: "" };

/** The reason is inside the hashed payload and bounded by the plane (payments.proto). */
export const REASON_MAX_BYTES = 500;

const BYTES = new TextEncoder();

export function reasonBytes(reason: string): number {
  return BYTES.encode(reason).length;
}

/** The two internal kinds that name one of many, and so need an id picked. */
export function needsId(kind: EndKind): boolean {
  return kind === "service" || kind === "user";
}

export function previewTier(destination: EndDraft): PaymentTier {
  if (destination.kind === "external") return "external";
  if (destination.kind === "service") return "service";
  return "internal";
}

/** Decided by the source alone: an investor's own claim asks that investor, all else the owners. */
export function previewRequirement(source: EndDraft): PaymentRequirement {
  return source.kind === "user" ? "subject_consent" : "owner_consilium";
}

function complete(end: EndDraft): boolean {
  if (end.kind === "external") return end.network.trim().length > 0 && end.address.trim().length > 0;
  return !needsId(end.kind) || end.id.trim().length > 0;
}

function sameParty(a: EndDraft, b: EndDraft): boolean {
  if (a.kind === "external" || b.kind === "external" || a.kind !== b.kind) return false;
  return !needsId(a.kind) || a.id.trim() === b.id.trim();
}

/**
 * Why the draft cannot be sent yet, as a catalogue key — or null when it can.
 *
 * Mirrors the plane's own refusals so the operator hears about them before spending a
 * round trip, in the order they would notice them. The plane still decides: an external
 * destination from the two pooled claims is refused there too (`WithdrawalSource` has no
 * such rail), and this only says so a click earlier.
 */
export function draftProblem(source: EndDraft, destination: EndDraft, amount: string, reason: string): string | null {
  if (!complete(source)) return "admin.payments.err.pickSource";
  if (!complete(destination)) return "admin.payments.err.pickDestination";
  if (sameParty(source, destination)) return "admin.payments.err.sameEnds";
  if (destination.kind === "external" && (source.kind === "piggybank" || source.kind === "service")) return "admin.payments.err.noRailFromPooled";
  const n = Number(amount.trim());
  if (!amount.trim() || !Number.isFinite(n) || n <= 0) return "admin.payments.err.enterAmount";
  if (!reason.trim()) return "admin.payments.err.enterReason";
  if (reasonBytes(reason.trim()) > REASON_MAX_BYTES) return "admin.payments.err.reasonTooLong";
  return null;
}

function toParty(end: EndDraft): Party {
  // The singletons carry no id on the wire; sending a stale one from a previous pick
  // would name a product on a claim that has none.
  return { kind: end.kind === "external" ? "piggybank" : end.kind, id: needsId(end.kind) ? end.id.trim() : "" };
}

/** The wire request for a draft `draftProblem` has passed. Trimmed once, here. */
export function toRequest(source: EndDraft, destination: EndDraft, amount: string, reason: string): OpenPaymentRequest {
  return {
    source: toParty(source),
    destination:
      destination.kind === "external"
        ? { external: { network: destination.network.trim(), address: destination.address.trim() } }
        : { internal: toParty(destination) },
    amount: amount.trim(),
    reason: reason.trim(),
  };
}
