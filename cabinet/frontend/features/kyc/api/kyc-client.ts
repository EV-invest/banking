"use client";

// Browser → shell KYC client (site-root `/api/kyc/start`). Verification is identity-plane
// work and the identity plane is shell-owned, exactly like `/api/auth/sessions` — hence
// `scope: "shell"`, no zone prefix. Transport, CSRF and the session pre-flight/replay
// belong to `@/shared/lib/api-client`; only the answer-shaping is here.

import { RequestError, requestJson } from "@/shared/lib/api-client";

import { readString } from "@/features/kyc/lib/read-field";

/** The one code `/kyc/start` publishes. Switched on instead of its prose, by its own contract. */
const UNAVAILABLE = "kyc_unavailable";

/**
 * `unavailable` is its own outcome rather than a failure because "no vendor will open a
 * session" is a DESIGNED state of that route: it degrades to one 503 with a support address,
 * and the screen owes the user a different sentence for it than for a fault. Modelling it as
 * a thrown error would have the UI reconstructing the distinction from a status code.
 *
 * `stale` and `throttled` are here for the same reason and no other: both arrive as bare
 * status codes with plain-text bodies, so nothing downstream could tell them apart from an
 * ordinary failure. Everything that DOES carry a code stays inside `failed` and is worded
 * by `errorMessage` — this layer names outcomes, it does not write sentences.
 */
export type KycStart =
  | { kind: "started"; redirectUrl: string }
  | { kind: "unavailable"; contact: string | null }
  | { kind: "stale" }
  | { kind: "throttled" }
  | { kind: "failed"; error: unknown };

export async function startVerification(): Promise<KycStart> {
  try {
    // No body on purpose: absent `tier` means the entry tier, and that policy is the
    // identity plane's. Restating `{ tier: 1 }` here would be a second place to change it.
    const data = await requestJson<unknown>("/api/kyc/start", { method: "POST", scope: "shell" });
    const url = providerUrl(readString(data, "redirect_url"));
    return url ? { kind: "started", redirectUrl: url } : { kind: "failed", error: unusable() };
  } catch (error) {
    return classify(error);
  }
}

/**
 * The vendor's URL is handed to us by our own backend, but it lands in `window.location`,
 * where a `javascript:` or `data:` scheme executes in THIS origin. One scheme check costs
 * nothing and keeps a compromised or confused provider from becoming an XSS in the cabinet.
 */
function providerUrl(raw: string | null): string | null {
  if (!raw) return null;
  try {
    return new URL(raw).protocol === "https:" ? raw : null;
  } catch {
    return null;
  }
}

/**
 * A 200 we cannot act on. Shaped as a `RequestError` with status 0 — the transport's own
 * idiom for "no HTTP verdict", as used for a network failure — so the row renders every
 * outcome through the one `errorMessage` boundary instead of carrying prose of its own.
 */
function unusable(): RequestError {
  return new RequestError("We couldn't start verification. Please try again.", 0, "err.kycStartFailed");
}

function classify(error: unknown): KycStart {
  if (error instanceof RequestError) {
    if (error.status === 503 && readString(error.body, "error") === UNAVAILABLE) {
      return { kind: "unavailable", contact: readString(error.body, "contact") };
    }
    // The daily cap on starts, refused in plain text with no machine-readable code, so the
    // status is all there is to key on. The transport's generic wording ("give it a moment")
    // is wrong for a 24-hour window — the screen owes this one its own sentence.
    if (error.status === 429) return { kind: "throttled" };
    // Keyed on the ABSENCE of a body code, not on the status alone. The only 403 this plane
    // raises today is a stale token, refused as the plain text `csrf check failed` — there is
    // no `{ error: "csrf" }` to match, and the transport's generic 403 wording ("You don't
    // have access to this") would send the user hunting for a permission problem instead of
    // reloading. But a 403 that DOES name its reason is a refusal on the merits, and telling
    // that user the page went stale sends them reloading forever instead of to support.
    if (error.status === 403 && readString(error.body, "error") === null) return { kind: "stale" };
  }
  // Includes SessionExpiredError, whose own code already says to sign in again.
  return { kind: "failed", error };
}
