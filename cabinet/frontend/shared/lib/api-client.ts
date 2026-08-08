"use client";

// The one browser→BFF JSON transport. Every entity client goes through it, so session
// handling and error wording live HERE instead of in six copies of `fetch`-and-throw.
//
// Why it does more than fetch: the shared access cookie lapses on a short TTL while the
// page still looks signed in (see `shared/lib/session.ts`). Left alone, a tab that idled
// past that TTL renders fine and then fails every call with a bare `unauthenticated` —
// which is what the user saw, and why "click around a few times or reload" appeared to
// fix it. Two layers close that:
//
//   · pre-flight — a request issued after the session went unconfirmed for STALE_MS
//     rotates the cookie FIRST (coalesced with the session fetch the shell chrome is
//     already making), so coming back to an idle tab never produces a 401 at all;
//   · heal — a 401 that slips through anyway (the cookie lapsed mid-page, or a request
//     raced the pre-flight) refreshes the session and replays the request ONCE. Replaying
//     is safe for mutations too: a 401 is only ever an auth verdict, raised by the BFF's
//     gate before any upstream call (see backend/src/routes) or by the owning plane's own
//     gate before it applies anything — never by a request that half-executed.
//
// Only a shell that answers "genuinely signed out" ends as SessionExpiredError (the
// keeper picks the same answer up and moves to /login). An unreachable session endpoint
// is left as an ordinary failure, so a blip never bounces anyone out of the cabinet.

import { apiPath } from "@/shared/config/base-path";
import { csrfHeader } from "@/shared/lib/csrf-client";
import { cachedSession, refreshIfStale, refreshSession, sessionGeneration } from "@/shared/lib/session";

/** The session is provably gone — offer sign-in, not a retry. */
export class SessionExpiredError extends Error {
  readonly status = 401;
  constructor() {
    super("Your session has ended. Sign in again to continue.");
    this.name = "SessionExpiredError";
  }
}

/** A failed BFF call, carrying the HTTP status for callers that branch on it. */
export class RequestError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
    this.name = "RequestError";
  }
}

type Method = "GET" | "POST" | "PATCH" | "DELETE";

interface JsonRequest {
  method?: Method;
  body?: unknown;
  /** `zone` = this cabinet's BFF (basePath-prefixed); `shell` = the site-root auth surface. */
  scope?: "zone" | "shell";
}

// The BFF's fixed error strings, in the user's words. Everything else the BFF sends is
// already client-safe prose (see backend/src/error.rs) and passes through untouched.
const FRIENDLY: Record<string, string> = {
  unauthenticated: "We couldn't confirm your session. Reload the page or sign in again.",
  csrf: "This page went stale. Reload it and try again.",
  "auth not configured": "Sign-in is unavailable right now. Please try again shortly.",
  "request failed": "Something went wrong on our side. Please try again.",
};

// Fallbacks when the response carries no `{ error }` body at all.
function statusMessage(status: number): string {
  if (status === 403) return "You don't have access to this.";
  if (status === 404) return "Not found.";
  if (status === 429) return "Too many requests — give it a moment and try again.";
  if (status >= 500) return "The service is temporarily unavailable. Please try again.";
  return `Request failed (${status}).`;
}

export async function requestJson<T>(path: `/${string}`, req: JsonRequest = {}): Promise<T> {
  const { method = "GET", body, scope = "zone" } = req;
  const url = scope === "zone" ? apiPath(path) : path;
  const init: RequestInit = {
    method,
    headers:
      body === undefined
        ? { accept: "application/json", ...(method === "GET" ? {} : csrfHeader()) }
        : { accept: "application/json", "content-type": "application/json", ...csrfHeader() },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  };

  // Pre-flight: rotate a cookie that is old enough to be at risk before spending the
  // request on it. Cheap — single-flight with every other session reader on the page.
  await refreshIfStale();
  const generation = sessionGeneration();

  let res = await send(url, init);
  if (res.status === 401) {
    // Heal: if the shell already answered while this request was in flight, the replay
    // just uses the cookie that answer re-set; otherwise refresh first.
    const session = sessionGeneration() > generation ? cachedSession() : await refreshSession();
    if (session && !session.authenticated) throw new SessionExpiredError();
    res = await send(url, init);
  }

  const data = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) {
    const detail = data.error ? (FRIENDLY[data.error] ?? data.error) : statusMessage(res.status);
    throw new RequestError(detail, res.status);
  }
  return data;
}

/** Network-level failure → the same friendly surface as an error response. */
async function send(url: string, init: RequestInit): Promise<Response> {
  try {
    return await fetch(url, init);
  } catch {
    throw new RequestError("Can't reach the server. Check your connection and try again.", 0);
  }
}

export const getJson = <T>(path: `/${string}`): Promise<T> => requestJson<T>(path);

export const postJson = <T>(path: `/${string}`, body: unknown): Promise<T> => requestJson<T>(path, { method: "POST", body });
