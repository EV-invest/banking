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
// when there is no value — and every helper that DOES narrow it (`ownerCount`,
// `removalCandidates`, `seatedOwnerIds`) answers `null` rather than a zero or an empty
// list, so a caller that forgets the case renders nothing instead of a claim.
//
// The rule extends to everything this page grew afterwards. Admissions are read through
// the same `Read` — "no admission is open" is a claim about who is being let into the fund
// and a read that failed is not entitled to make it — and `elevatedWithoutSeat` carries it
// off this page entirely: the admin users table marks a row as seatless only against a
// roster that actually arrived, and marks nothing at all when the roster is forbidden or
// down.
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
 * What the roster actually says, as four answers the page renders differently.
 *
 * `unseated` is the one this exists for. An empty roster used to be treated as impossible
 * — the copy told the reader to report it as a fault — and that was true only while every
 * account with the `Owner` label was a seated owner. It no longer is: a subject listed in
 * `ADMIN_SUBJECTS` is promoted to `Owner` in the directory at request time and never
 * persisted, and the roster is read from the PERSISTED role precisely so that a seat
 * cannot be granted by whoever can edit an environment variable (docs/CONSILIUM.md, the
 * residual under § Admission). A fund whose only elevated accounts are break-glass ones
 * therefore has a legitimately empty roster, and telling that reader to file a bug is
 * worse than telling them nothing.
 *
 * It stays a separate answer from `failed`, which is the distinction the rest of this file
 * is about: "nobody holds a seat" and "we could not find out" are different sentences, and
 * only one of them is a fault.
 */
export type RosterState = "loading" | "failed" | "unseated" | "seated";

export function rosterState(roster: Read<{ items?: readonly unknown[] }>): RosterState {
  if (roster.status === "loading") return "loading";
  if (roster.status === "failed") return "failed";
  return (roster.value.items ?? []).length === 0 ? "unseated" : "seated";
}

/**
 * The ids that actually hold a seat, or `null` when the roster is not known.
 *
 * `null` is load-bearing at the call site: it means "no cross-reference is possible", and
 * a caller that marks nothing on `null` degrades into the screen it had before. Answering
 * an empty set instead would claim every elevated account is unseated on the strength of a
 * read that never landed — which is the same class of lie as "0 owners", pointed at a
 * different screen.
 */
export function seatedOwnerIds<T extends { user_id: string }>(roster: Read<{ items?: readonly T[] }>): ReadonlySet<string> | null {
  if (roster.status !== "ready") return null;
  return new Set((roster.value.items ?? []).map((owner) => owner.user_id));
}

/**
 * Whether an account carrying the owner role holds no seat in the consilium.
 *
 * This is the contradiction the two screens were publishing at once, and both halves were
 * correct. The directory applies `effective_role`, promoting every `ADMIN_SUBJECTS`
 * subject to `Owner` without persisting it, and reports the promoted role from `ListUsers`
 * — so the admin table shows "Owner". The consilium roster reads the persisted role, so it
 * shows nobody. Rather than reconciling them by counting break-glass admins as owners —
 * which would reopen the exact hole the mechanism exists to close — the row is marked for
 * what it is: elevated access, no seat.
 *
 * `seated === null` answers `false`, deliberately and in the quiet direction: with no
 * roster to compare against, an unmarked row is a row that says nothing, whereas a marked
 * one would accuse a real owner of holding no seat.
 */
export function elevatedWithoutSeat(user: { user_id: string; role?: string | null }, seated: ReadonlySet<string> | null): boolean {
  if (seated === null) return false;
  if ((user.role ?? "").trim().toLowerCase() !== "owner") return false;
  return !seated.has(user.user_id);
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
