// The proxy's session decision, without the proxy.
//
// `proxy.ts` imports `next/server` and so cannot be loaded by the test runner. Everything
// it decides about the session — which cookies the forwarded request carries, which
// Set-Cookie lines reach the browser, and whether to bounce to /login or off it — is
// decided here from plain values, so that decision can be pinned without a Next runtime.
// Relative imports only: the runner resolves no `@/` alias.

import { BASE_PATH, zonePathname } from "../config/base-path.ts";
import { isPublicPath, zoneGatePath } from "../config/public-routes.ts";
import type { Renewal } from "./access-renewal.ts";

export interface SessionGateInput {
  pathname: string;
  /** The query string as the request carried it, `?` included, or "". */
  search: string;
  /** The locale segment of the path, or null when there is none. */
  locale: string | null;
  /** Whether the request carries a non-empty session cookie. */
  hasSession: boolean;
  /** The renewal attempt's outcome, or null when none was made. */
  renewal: Renewal | null;
  cookieNames: { session: string; access: string };
}

export interface SessionDecision {
  /** Where to send the browser instead of rendering, or null to render. */
  redirect: { pathname: string; returnTo: string | null } | null;
  /** Cookie values to set on the forwarded request (the renewed access token). */
  requestCookies: Readonly<Record<string, string>>;
  /** Cookies to remove from the forwarded request (a session the identity plane says is gone). */
  deleteCookies: readonly string[];
  /** Set-Cookie lines for the response, verbatim from the identity plane. */
  setCookies: readonly string[];
}

export function decideSession({ pathname, search, locale, hasSession, renewal, cookieNames }: SessionGateInput): SessionDecision {
  const requestCookies: Record<string, string> = {};
  const deleteCookies: string[] = [];
  let signedIn = hasSession;
  if (renewal?.kind === "alive" && renewal.access) requestCookies[cookieNames.access] = renewal.access;
  if (renewal?.kind === "gone") {
    // Same as arriving without a session: the cleared cookies go out with the bounce.
    signedIn = false;
    deleteCookies.push(cookieNames.session, cookieNames.access);
  }
  const setCookies = renewal && renewal.kind !== "unknown" ? renewal.setCookies : [];
  const base = { requestCookies, deleteCookies, setCookies };

  if (!isPublicPath(pathname) && !signedIn) {
    // Zone-relative, like `SessionKeeper`'s: the login view puts `/{locale}/cabinet` back
    // on when it hands returnTo to the shell. Passing the real path here doubled the
    // prefix and landed every deep link on `/cabinet/{locale}/cabinet/…` (#390).
    const returnTo = `${zonePathname(pathname)}${search}`;
    return { ...base, redirect: { pathname: `/${locale ?? "en"}${BASE_PATH}/login`, returnTo: returnTo === "/" ? null : returnTo } };
  }
  if (signedIn && zoneGatePath(pathname) === "/login") {
    return { ...base, redirect: { pathname: `/${locale ?? "en"}${BASE_PATH}`, returnTo: null } };
  }
  return { ...base, redirect: null };
}
