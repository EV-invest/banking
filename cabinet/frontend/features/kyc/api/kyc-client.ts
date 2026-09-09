"use client";

// Browser → shell KYC client (site-root `/api/kyc/start`). Verification is identity-plane
// work and the identity plane is shell-owned, exactly like `/api/auth/sessions` — hence
// `scope: "shell"`, no zone prefix. Transport, CSRF and the session pre-flight/replay
// belong to `@/shared/lib/api-client`; only the answer-shaping is here.

import { RequestError, requestJson, STALE_PAGE_MESSAGE } from "@/shared/lib/api-client";

import { readString } from "@/features/kyc/lib/read-field";

/** The one code `/kyc/start` publishes. Switched on instead of its prose, by its own contract. */
const UNAVAILABLE = "kyc_unavailable";

const UNUSABLE = "We couldn't start verification. Please try again.";

/**
 * Three outcomes, not two, because "no vendor will open a session" is a DESIGNED state of
 * that route rather than a fault: it degrades to one 503 with a support address, and the
 * screen owes the user a different sentence for it than for a failure. Modelling it as a
 * thrown error would have the UI reconstructing the distinction from a status code.
 */
export type KycStart =
  | { kind: "started"; redirectUrl: string }
  | { kind: "unavailable"; contact: string | null }
  | { kind: "failed"; message: string };

export async function startVerification(): Promise<KycStart> {
  try {
    // No body on purpose: absent `tier` means the entry tier, and that policy is the
    // identity plane's. Restating `{ tier: 1 }` here would be a second place to change it.
    const data = await requestJson<unknown>("/api/kyc/start", { method: "POST", scope: "shell" });
    const url = providerUrl(readString(data, "redirect_url"));
    return url ? { kind: "started", redirectUrl: url } : { kind: "failed", message: UNUSABLE };
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

function classify(error: unknown): KycStart {
  if (error instanceof RequestError) {
    if (error.status === 503 && readString(error.body, "error") === UNAVAILABLE) {
      return { kind: "unavailable", contact: readString(error.body, "contact") };
    }
    // Keyed on the body, not the status: a 403 here is a stale token ONLY when the body
    // says so. Every other 403 this route can answer — a suspended account, an unserved
    // country, a policy refusal — is a substantive verdict and must carry its own message
    // instead of being told to reload. If this plane ever starts sending `{ error: "csrf" }`
    // like the rest of the transport's table expects, this still matches it; until then a
    // plain-text CSRF refusal falls through to `error.message` below along with every other
    // 403, which is the transport's honest (if generic) "You don't have access to this."
    if (error.status === 403 && readString(error.body, "error") === "csrf") {
      return { kind: "failed", message: STALE_PAGE_MESSAGE };
    }
    return { kind: "failed", message: error.message };
  }
  // Includes SessionExpiredError, whose own message already says to sign in again.
  return { kind: "failed", message: error instanceof Error ? error.message : UNUSABLE };
}
