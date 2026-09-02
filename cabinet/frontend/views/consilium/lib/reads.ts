// What this page is entitled to say.
//
// A read that has not arrived is not a read that came back empty, and on a governance
// surface the difference is the whole point. The page lost it once: `owners.data?.items ??
// []` turned a 404 into an empty roster, and two lines further on that empty roster was
// printed as "0 owners" in the card header and as "There is nobody to propose — you are the
// only owner listed" in the form. Both are statements about who controls the fund, made
// from a read that never landed. An owner who believes either one has been told something
// false about the very thing this page exists to be honest about.
//
// So nothing here reads `.data` and falls back. It reads a `Read<T>`, which cannot be
// turned into a value without the call site saying, in the same expression, what it does
// when there is no value — and the two helpers that DO narrow it (`ownerCount`,
// `removalCandidates`) answer `null` rather than a zero or an empty list, so a caller that
// forgets the case renders nothing instead of a claim.
//
// Pure, structurally typed and import-free on purpose: this is the rule the page is judged
// on, so it is the part that has to be testable outside a browser (`./reads.test.ts`).

export type Read<T> =
  | { status: "loading" }
  | { status: "failed"; error: Error }
  | { status: "ready"; value: T };

/** The part of a `ResourceSnapshot` (`shared/lib/resource.ts`) this derivation reads. */
export interface ReadSource<T> {
  data: T | undefined;
  error: Error | null;
}

/**
 * Which of the three a snapshot is in.
 *
 * A value present alongside an error is still `ready`, deliberately: that pair means a
 * *refresh* failed over figures the reader can already see, and blanking them because one
 * poll timed out is the failure the cache is built to avoid. It is a `failed` read only
 * when there is nothing to show in its place.
 */
export function readOf<T>(source: ReadSource<T>): Read<T> {
  if (source.data !== undefined) return { status: "ready", value: source.data };
  if (source.error !== null) return { status: "failed", error: source.error };
  return { status: "loading" };
}

/** Derive from what arrived, carrying "nothing arrived" through untouched. */
export function mapRead<T, U>(read: Read<T>, project: (value: T) => U): Read<U> {
  return read.status === "ready" ? { status: "ready", value: project(read.value) } : read;
}

/**
 * The value, or `null` when the read has not landed — the only door out of a `Read`.
 *
 * `?? []` on the result is exactly as wrong as it was before; the difference is that it now
 * has to be written out at the call site instead of hiding inside an optional chain.
 */
export function knownValue<T>(read: Read<T>): T | null {
  return read.status === "ready" ? read.value : null;
}

/**
 * How many owners there are, or `null` when that is not known.
 *
 * Never `0` for a read that failed or is still in flight. "0 owners" is a fact about the
 * fund, and this page is not entitled to state it unless the roster actually arrived.
 */
export function ownerCount(roster: Read<{ items?: readonly unknown[] }>): number | null {
  // `items ?? []` here is not the mistake this file is about, and the difference is the
  // whole distinction: proto3 JSON omits an empty repeated field, so an ABSENT list on a
  // read that arrived means the fund really has none (`shared/contracts/governance.ts`).
  // A read that did not arrive has already been answered `null` on the line above.
  return roster.status === "ready" ? (roster.value.items ?? []).length : null;
}

/**
 * The owners a removal could be proposed against, or `null` when the roster is not known.
 *
 * The empty array and `null` are different answers and the form renders them differently:
 * `[]` means the fund really does have nobody else in it, `null` means the page has no idea
 * — and only the first of those may say "you are the only owner".
 */
export function removalCandidates<T extends { user_id: string }>(
  roster: Read<{ items?: readonly T[] }>,
  userId: string | null,
): T[] | null {
  if (roster.status !== "ready") return null;
  return (roster.value.items ?? []).filter((owner) => owner.user_id !== userId);
}

/**
 * Every one of these reads failed — one cause, so one failure to report.
 *
 * False as soon as one of them is loading or has landed. "The roster loaded but the payouts
 * did not" is real information about which half of the room can be trusted, and folding a
 * partial failure into a page-level banner throws it away.
 */
export function everyReadFailed(reads: readonly Read<unknown>[]): boolean {
  return reads.length > 0 && reads.every((read) => read.status === "failed");
}
