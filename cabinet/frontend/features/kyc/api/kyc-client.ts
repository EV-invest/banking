"use client";

// Browser → shell KYC client (site-root `/api/kyc/start`, `/api/kyc/status`). Verification is
// identity-plane work and the identity plane is shell-owned, exactly like `/api/auth/sessions`
// — hence `scope: "shell"`, no zone prefix. Transport, CSRF and the session pre-flight/replay
// belong to `@/shared/lib/api-client`; the wire shapes belong to ./kyc-contract; only the
// answer-shaping — which wire answer is which OUTCOME for a screen — is here.

import { config } from "@/config";
import { parseErrorBody, parseStartResponse, parseStatus, type KycStatus } from "@/features/kyc/api/kyc-contract";
import { providerUrl } from "@/features/kyc/lib/provider-url";
import { readString } from "@/features/kyc/lib/read-field";
import { RequestError, requestJson } from "@/shared/lib/api-client";

/**
 * `unavailable` is its own outcome rather than a failure because "no vendor will open a
 * session" is a DESIGNED state of that route: it degrades to one 503 with a support address,
 * and the screen owes the user a different sentence for it than for a fault. Modelling it as
 * a thrown error would have the UI reconstructing the distinction from a status code.
 *
 * `stale` and `throttled` are here for the same reason and no other: neither has wording of
 * its own in the transport's table that fits — a 24-hour cap is not "give it a moment", and a
 * stale CSRF token is not "you don't have access to this". Everything that carries a code the
 * transport already words stays inside `failed` — this layer names outcomes, it does not
 * write sentences.
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
    const started = parseStartResponse(data);
    const url = providerUrl(started?.redirectUrl ?? null, config.public.kycProviderHost);
    return url ? { kind: "started", redirectUrl: url } : { kind: "failed", error: unusable() };
  } catch (error) {
    return classify(error);
  }
}

/**
 * The caller's tier and their running case, or `null` when the plane cannot tell us.
 *
 * `null` rather than a throw because every consumer of this answers the same way — fall back
 * to the tier the profile already carries — and because the commonest reason for it is not an
 * error at all: a cabinet deployed ahead of the concierge release that adds the route gets a
 * 404 here, and must keep working exactly as it does today.
 */
export async function fetchKycStatus(): Promise<KycStatus | null> {
  try {
    // A GET: no CSRF token, and nothing is spent by asking.
    return parseStatus(await requestJson<unknown>("/api/kyc/status", { scope: "shell" }));
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
  if (!(error instanceof RequestError)) {
    // Includes SessionExpiredError, whose own code already says to sign in again.
    return { kind: "failed", error };
  }
  // Keyed on the body's code, not on the status: the plane publishes a closed dictionary and
  // answers every refusal with one, so the status is now corroboration rather than evidence.
  // (This retires the old "a 403 with no body is a stale token" heuristic — there is no
  // bodyless refusal left for it to match.)
  const body = parseErrorBody(error.body);
  switch (body?.error) {
    case "kyc_unavailable":
      return { kind: "unavailable", contact: body.contact };
    case "throttled":
      return { kind: "throttled" };
    case "csrf":
      return { kind: "stale" };
    // `internal` is the one code the transport's own table does not word, so it would reach
    // the reader as the literal string "internal". Re-keyed to the sentence every other 5xx
    // in the cabinet gets.
    case "internal":
      return { kind: "failed", error: new RequestError("The service is temporarily unavailable. Please try again.", error.status, "err.serverUnavailable", error.body) };
    // `unauthenticated` IS in that table (`err.unauthenticated`), and a session the shell
    // confirms is gone has already been turned into SessionExpiredError upstream.
    case "unauthenticated":
      return { kind: "failed", error };
    default:
      break;
  }
  // No code we know: fall back to what the status alone can say, so a plane that has not
  // shipped the dictionary yet still gets the outcomes that have their own wording. The 403
  // branch keeps the old rule — a refusal that NAMES a reason is a refusal on the merits, and
  // telling that reader the page went stale sends them reloading forever instead of to
  // support; only a bodyless one is the identity plane's plain-text `csrf check failed`.
  if (error.status === 429) return { kind: "throttled" };
  if (error.status === 403 && readString(error.body, "error") === null) return { kind: "stale" };
  return { kind: "failed", error };
}
