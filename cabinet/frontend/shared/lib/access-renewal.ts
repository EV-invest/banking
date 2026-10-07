// Server-side renewal of the shell's access cookie, for `proxy.ts`.
//
// Why the proxy does this at all. A server render forwards `ev_access` to the BFF, and the
// JWT inside lapses on a short TTL (300 s deployed) while its cookie lives for the whole
// refresh window — so a tab reopened after a coffee break arrives with a cookie that is
// present and dead. The browser's `SessionKeeper` heals that, but only after hydration,
// which is too late for data the server already tried to read. Renewing here, before the
// render, lets the server read succeed on the first try.
//
// The concierge `GET /auth/session` endpoint is the only thing that renews it, and it is
// safe to call concurrently: it serialises per session and hands back the current token
// without rotating the (single-use) refresh token while the access token still has more
// than 30 s left. The single-flight below only spares it the duplicate calls one burst of
// requests (a page plus its prefetches) would make.
//
// No `next/server` import, so the test runner can reach every branch here.

/** Seconds of remaining life under which the proxy renews before rendering. */
export const RENEW_WITHIN_S = 60;

/** How long a page request may wait on the identity plane before going on without it. */
export const RENEW_TIMEOUT_MS = 1_500;

/**
 * After a call that got no verdict, how long every request skips renewal. A plane that is
 * down or unreachable (a dropped packet waits out the whole timeout) would otherwise add
 * `RENEW_TIMEOUT_MS` to every page with a lapsing cookie; this caps it at one such page per
 * window per process. The browser's SessionKeeper renews in the meantime.
 */
export const RENEW_BACKOFF_MS = 30_000;

/**
 * Seconds until the JWT's `exp`, or null when the value is not a readable JWT.
 *
 * The signature is NOT checked and does not need to be: this only decides whether to ask
 * the identity plane for a new token. The BFF verifies whatever is finally sent.
 */
export function secondsUntilExpiry(token: string, nowMs: number): number | null {
  const payload = token.split(".")[1];
  if (!payload) return null;
  try {
    const json: unknown = JSON.parse(atob(payload.replaceAll("-", "+").replaceAll("_", "/")));
    if (typeof json !== "object" || json === null || !("exp" in json) || typeof json.exp !== "number") return null;
    return json.exp - Math.floor(nowMs / 1000);
  } catch {
    return null;
  }
}

/** Whether a request carrying this access cookie (or none) should be renewed first. */
export function needsRenewal(access: string | undefined, nowMs: number): boolean {
  if (!access) return true;
  const left = secondsUntilExpiry(access, nowMs);
  return left === null || left < RENEW_WITHIN_S;
}

/** The value a `Set-Cookie` line assigns to `name`, or null when it sets another cookie. */
export function setCookieValue(line: string, name: string): string | null {
  const pair = line.split(";", 1)[0] ?? "";
  const eq = pair.indexOf("=");
  if (eq === -1 || pair.slice(0, eq).trim() !== name) return null;
  return pair.slice(eq + 1).trim();
}

export type Renewal =
  /** Signed in. `access` is the token to render with, or null if none was re-set. */
  | { kind: "alive"; access: string | null; setCookies: readonly string[] }
  /** The identity plane says the session is gone; `setCookies` clear the cookies. */
  | { kind: "gone"; setCookies: readonly string[] }
  /** No verdict (timeout, 5xx, network) — go on without renewing; the browser heals later. */
  | { kind: "unknown" };

export interface RenewalRequest {
  /** Base URL of the concierge web surface (`AUTH_WEB_URL`). */
  authWebUrl: string;
  sessionCookie: { name: string; value: string };
  accessCookieName: string;
  timeoutMs?: number;
  fetch?: typeof fetch;
}

const UNKNOWN: Renewal = Object.freeze({ kind: "unknown" });

const inflight = new Map<string, Promise<Renewal>>();
let backoffUntil = 0;

/**
 * Ask the identity plane for the session's current access token. Never rejects.
 *
 * Concurrent calls for the same session share one request, so a burst from one browser
 * costs the identity plane one call per process rather than one per request.
 */
export function renewAccess(request: RenewalRequest, nowMs = Date.now()): Promise<Renewal> {
  if (nowMs < backoffUntil) return Promise.resolve(UNKNOWN);
  const key = request.sessionCookie.value;
  const existing = inflight.get(key);
  if (existing) return existing;
  const pending = callSession(request)
    .then((renewal) => {
      if (renewal.kind === "unknown") backoffUntil = Date.now() + RENEW_BACKOFF_MS;
      return renewal;
    })
    .finally(() => inflight.delete(key));
  inflight.set(key, pending);
  return pending;
}

async function callSession({ authWebUrl, sessionCookie, accessCookieName, timeoutMs = RENEW_TIMEOUT_MS, fetch: fetchImpl = fetch }: RenewalRequest): Promise<Renewal> {
  let res: Response;
  try {
    res = await fetchImpl(`${authWebUrl.replace(/\/+$/, "")}/auth/session`, {
      // Only the session id: the endpoint needs nothing else, and nothing else of the
      // browser's should leave this process.
      headers: { accept: "application/json", cookie: `${sessionCookie.name}=${sessionCookie.value}` },
      cache: "no-store",
      redirect: "manual",
      signal: AbortSignal.timeout(timeoutMs),
    });
  } catch {
    return UNKNOWN;
  }
  if (!res.ok) return UNKNOWN;
  let body: unknown;
  try {
    body = await res.json();
  } catch {
    return UNKNOWN;
  }
  if (typeof body !== "object" || body === null || !("authenticated" in body) || typeof body.authenticated !== "boolean") return UNKNOWN;
  const setCookies = res.headers.getSetCookie();
  if (!body.authenticated) return { kind: "gone", setCookies };
  const access = setCookies.map((line) => setCookieValue(line, accessCookieName)).find((value) => value) ?? null;
  return { kind: "alive", access, setCookies };
}

/** Test seam: forget in-flight renewals and any backoff so each case starts cold. */
export function resetRenewalsForTests(): void {
  inflight.clear();
  backoffUntil = 0;
}
