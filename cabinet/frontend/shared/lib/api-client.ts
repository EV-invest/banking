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
  readonly code = "err.sessionExpired";
  constructor() {
    super("Your session has ended. Sign in again to continue.");
    this.name = "SessionExpiredError";
  }
}

/**
 * A failed BFF call, carrying the HTTP status for callers that branch on it.
 *
 * `code` is a catalogue key, not prose — this module runs in the fetch layer,
 * far below any component that could hold a translator, so it names the message
 * and leaves the wording to the render boundary (see {@link errorMessage}).
 * `message` stays populated with the English text so a `console.error`, a Sentry
 * breadcrumb, or a caller that never learned about `code` still reads sensibly.
 *
 * A `code` of `null` means the BFF sent free-form prose of its own. That text is
 * already client-safe (see backend/src/error.rs) but it is not ours to key, so it
 * passes through in whatever language the backend chose.
 */
export class RequestError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly code: string | null = null,
  ) {
    super(message);
    this.name = "RequestError";
  }
}

/**
 * The one place an error becomes words a reader sees.
 *
 * Views render this rather than `error.message`: the message was fixed in English
 * when the request failed, whereas the key can still be resolved in the reader's
 * locale at paint time. Anything that is not one of our errors — a thrown string,
 * a bug — degrades to the generic key rather than leaking a stack trace into the UI.
 */
export function errorMessage(error: unknown, t: (key: string) => string): string {
  if (error instanceof SessionExpiredError) return t(error.code);
  if (error instanceof RequestError) return error.code ? t(error.code) : error.message;
  return t("err.requestFailed");
}

type Method = "GET" | "POST" | "PATCH" | "DELETE";

interface JsonRequest {
  method?: Method;
  body?: unknown;
  /** `zone` = this cabinet's BFF (basePath-prefixed); `shell` = the site-root auth surface. */
  scope?: "zone" | "shell";
}

// The BFF's fixed error strings, mapped to catalogue keys and their English text.
// Everything else the BFF sends is already client-safe prose (see backend/src/error.rs)
// and passes through untouched — with a null key, since we did not author it.
const FRIENDLY: Record<string, { code: string; en: string }> = {
  unauthenticated: {
    code: "err.unauthenticated",
    en: "We couldn't confirm your session. Reload the page or sign in again.",
  },
  csrf: { code: "err.csrf", en: "This page went stale. Reload it and try again." },
  "auth not configured": {
    code: "err.authNotConfigured",
    en: "Sign-in is unavailable right now. Please try again shortly.",
  },
  "request failed": {
    code: "err.requestFailed",
    en: "Something went wrong on our side. Please try again.",
  },
};

// Fallbacks when the response carries no `{ error }` body at all.
function statusMessage(status: number): { code: string; en: string } {
  if (status === 403) return { code: "err.forbidden", en: "You don't have access to this." };
  if (status === 404) return { code: "err.notFound", en: "Not found." };
  if (status === 429)
    return { code: "err.rateLimited", en: "Too many requests — give it a moment and try again." };
  if (status >= 500)
    return {
      code: "err.serverUnavailable",
      en: "The service is temporarily unavailable. Please try again.",
    };
  return { code: "err.requestFailed", en: `Request failed (${status}).` };
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
    // A BFF string we recognise becomes a key; one we don't stays the backend's own
    // prose, unkeyed. No `{ error }` body at all falls back to the status.
    const known = data.error ? FRIENDLY[data.error] : statusMessage(res.status);
    const detail = known ?? { code: null, en: data.error as string };
    throw new RequestError(detail.en, res.status, detail.code);
  }
  return data;
}

/** Network-level failure → the same friendly surface as an error response. */
async function send(url: string, init: RequestInit): Promise<Response> {
  try {
    return await fetch(url, init);
  } catch {
    throw new RequestError(
      "Can't reach the server. Check your connection and try again.",
      0,
      "err.network",
    );
  }
}

export const getJson = <T>(path: `/${string}`): Promise<T> => requestJson<T>(path);

export const postJson = <T>(path: `/${string}`, body: unknown): Promise<T> => requestJson<T>(path, { method: "POST", body });
