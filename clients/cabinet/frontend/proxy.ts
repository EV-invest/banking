import { type NextRequest, NextResponse } from "next/server";

import { createAbMiddleware } from "@evinvest/experiments/next";

import { experiments } from "@/application/experiments";
import { COOKIES } from "@/shared/config/cookies";
import { contentSecurityPolicy } from "@/shared/config/security";

const CSP_HEADER = "content-security-policy";

// A/B assignment boundary (Next 16 "proxy", formerly middleware; Node runtime).
// Assigns a sticky `ab_<key>` cookie per experiment in the registry on first
// visit. A no-op while `experiments` is empty.
const ab = createAbMiddleware(experiments);

// Pages reachable without a session. Everything else under the matcher requires the
// opaque session cookie: unauthenticated requests bounce to /login (carrying returnTo),
// and signed-in requests are kept off the auth pages. The cookie is only a cheap gate —
// the BFF still verifies the session server-side on every API call and page data fetch.
const PUBLIC = ["/login", "/loggedout"];

function isPublic(pathname: string): boolean {
  return PUBLIC.some((p) => pathname === p || pathname.startsWith(`${p}/`));
}

export function proxy(req: NextRequest) {
  const { pathname, search } = req.nextUrl;
  const signedIn = Boolean(req.cookies.get(COOKIES.session)?.value);

  // Per-request nonce: written onto the forwarded request headers so Next applies
  // it to its own inline bootstrap scripts (keeping script-src free of
  // 'unsafe-inline'), and echoed on the response so the browser enforces the CSP.
  const nonce = crypto.randomUUID().replaceAll("-", "");
  const csp = contentSecurityPolicy(nonce);
  req.headers.set(CSP_HEADER, csp);

  if (!isPublic(pathname) && !signedIn) {
    const url = req.nextUrl.clone();
    url.pathname = "/login";
    url.search = "";
    const returnTo = `${pathname}${search}`;
    if (returnTo !== "/") url.searchParams.set("returnTo", returnTo);
    return withCsp(NextResponse.redirect(url), csp);
  }

  if (signedIn && pathname === "/login") {
    const url = req.nextUrl.clone();
    url.pathname = "/";
    url.search = "";
    return withCsp(NextResponse.redirect(url), csp);
  }

  return withCsp(ab(req), csp);
}

function withCsp(res: NextResponse, csp: string): NextResponse {
  res.headers.set(CSP_HEADER, csp);
  return res;
}

export const config = {
  matcher: ["/((?!api|_next/static|_next/image|favicon.ico).*)"],
};
