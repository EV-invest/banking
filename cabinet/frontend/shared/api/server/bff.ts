import "server-only";

import { cookies } from "next/headers";

import { config } from "@/config";
import { COOKIES } from "@/shared/config/cookies";
import { isAccessToken } from "@/shared/lib/access-renewal";

// Server-render reads from the BFF — the server half of `shared/lib/api-client.ts`.
//
// A server component calls this to put real figures into the first HTML instead of a
// skeleton. It forwards only the access cookie (the one credential the BFF reads; renewed
// by `proxy.ts` when it was about to lapse) and never throws: any failure — no cookie, a
// 401, a timeout, a body of the wrong shape — is "no data", and the screen falls back to
// the browser read it has always made, which can still heal a session the server could
// not. A server read is an optimisation, never a new way for a screen to fail.

/** What a server read hands to a client resource as its `seed`. */
export interface ServerRead<T> {
  data: T;
  /** Epoch ms when the BFF answered — the resource's freshness clock starts here. */
  fetchedAt: number;
}

/** Long enough for a healthy BFF; short enough that a stuck one still lets the page settle. */
const READ_TIMEOUT_MS = 3_000;

/**
 * GET a BFF path (`/api/…`) with the caller's access cookie.
 *
 * `accept` narrows the parsed body; a body it rejects is reported as no data rather than
 * cast into a type it does not have.
 */
export async function bffRead<T>(path: `/api/${string}`, accept: (body: unknown) => body is T): Promise<ServerRead<T> | null> {
  // Next hands cookie values over percent-decoded; only a JWT-shaped value is safe to
  // write back into a Cookie header, where an encoded `;` would have become a separator.
  const access = (await cookies()).get(COOKIES.access)?.value;
  if (!isAccessToken(access)) return null;
  try {
    const res = await fetch(`${config.backendUrl.replace(/\/+$/, "")}${path}`, {
      headers: { accept: "application/json", cookie: `${COOKIES.access}=${access}` },
      cache: "no-store",
      signal: AbortSignal.timeout(READ_TIMEOUT_MS),
    });
    if (!res.ok) return null;
    const body: unknown = await res.json();
    return accept(body) ? { data: body, fetchedAt: Date.now() } : null;
  } catch {
    return null;
  }
}
