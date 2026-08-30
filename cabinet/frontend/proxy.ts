import { type NextRequest, NextResponse } from "next/server";

import { createAbMiddleware } from "@evinvest/experiments/next";
import { isLocale, negotiate, type Locale } from "@evinvest/i18n";

import { experiments } from "@/application/experiments";
import { config as appConfig } from "@/config";
import { BASE_PATH, isNonPagePath, localeRepairedPath } from "@/shared/config/base-path";
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

// Every page path is now `/{locale}/cabinet/…`, so the gate has to compare against
// what is left after that prefix. Getting this wrong fails open in the worse
// direction: an unrecognised path is treated as private, which bounces a signed-out
// reader to /login — annoying — rather than exposing a private page.
function withoutPrefix(pathname: string): string {
  const rest = pathname.replace(/^\/[a-z]{2}\/cabinet/, "");
  return rest === "" ? "/" : rest;
}

function isPublic(pathname: string): boolean {
  const path = withoutPrefix(pathname);
  return PUBLIC.some((p) => path === p || path.startsWith(`${p}/`));
}

/** The locale segment of `/{locale}/cabinet/…`, or null when there is none. */
function localeOf(pathname: string): Locale | null {
  const first = pathname.split("/")[1];
  return isLocale(first) ? first : null;
}

export function proxy(req: NextRequest) {
  const { pathname, search } = req.nextUrl;

  // Assets, the BFF and the MFE bundles are not pages: no locale redirect, no
  // session gate, no cookie writes. The matcher below excludes them as well, so
  // this looks redundant — it is not. When that exclusion silently stopped
  // matching (basePath removal, v0.2.57) it reached production and served every
  // stylesheet and script as the login page. Correctness lives here, where it is
  // unit-tested; the matcher is an optimisation that keeps this proxy off the
  // asset hot path.
  if (isNonPagePath(pathname)) return NextResponse.next();

  const signedIn = Boolean(req.cookies.get(COOKIES.session)?.value);
  const locale = localeOf(pathname);

  // Per-request nonce: written onto the forwarded request headers so Next applies
  // it to its own inline bootstrap scripts (keeping script-src free of
  // 'unsafe-inline'), and echoed on the response so the browser enforces the CSP.
  const nonce = crypto.randomUUID().replaceAll("-", "");
  const csp = contentSecurityPolicy(nonce);
  req.headers.set(CSP_HEADER, csp);

  // No usable locale in the path: an old link, a bookmark, the conductor's
  // unprefixed /cabinet mount, or a first segment that looks like a locale and is
  // not one (`/zz/cabinet/wallet`, `/pt/cabinet` — a typo, or a language we do not
  // serve). Send them all to a real URL rather than 404ing — the cookie is the
  // reader's last choice, Accept-Language the first guess, English the floor.
  //
  // The cookie is now written by the public site too (site_conductor
  // `scripts/locale-cookie.ts`), mirroring the locale of whatever page the reader
  // was on, so "their last choice" finally includes the language they were reading
  // on the landing a moment ago rather than only a previous cabinet visit.
  //
  // The `/<not-a-locale>/cabinet/…` half is why this is not a plain prefix test.
  // Those requests used to reach `app/[locale]/layout.tsx` and its `notFound()`,
  // which the ROOT layout throws and so has no `not-found.tsx` above it to render
  // — the reader got Next's built-in black 404 rather than ours. `localeRepairedPath`
  // covers both shapes, so repairing the URL fixes the page they land on as well as
  // the page they would otherwise be shown.
  if (!locale) {
    const remembered = req.cookies.get(COOKIES.locale)?.value;
    const chosen = isLocale(remembered)
      ? remembered
      : negotiate(req.headers.get("accept-language"));
    const repaired = localeRepairedPath(pathname, chosen);
    if (repaired) {
      const url = req.nextUrl.clone();
      url.pathname = repaired;
      const res = withCsp(NextResponse.redirect(url), csp);
      // Accept-Language is a header the reader never set, so a locale derived from it
      // is a guess, not a preference — and the account holder may well have stored a
      // real one from another device. The app cannot tell the two apart on its own:
      // this redirect carries no cookie, but the request it redirects TO does get one
      // written from the (guessed) URL by `withLocale` below, so by the time any page
      // renders the evidence is gone. Mark it here, while it is still known;
      // `LocaleSync` reads the mark, prefers the stored profile language over the
      // guess, and clears it. Session-scoped: a guess is only about this visit.
      //
      // A bogus locale segment is as much a guess as a missing one — `/pt/cabinet`
      // tells us which language the reader wanted and precisely that we do not serve
      // it — so it is marked on the same terms.
      //
      // The mark carries the guessed locale rather than a bare flag, so it can only
      // ever describe the page it was minted for. A flag would outlive its redirect
      // whenever the reader never reaches a page that clears it (the profile fetch
      // fails, or they land on /login, which mounts no LocaleSync) and would then
      // read as "this URL was guessed" on the next deliberate /de/cabinet link they
      // follow — bouncing them off the page they chose. Matching on the value makes
      // a stale mark inert instead.
      if (!isLocale(remembered)) {
        res.cookies.set(COOKIES.localeGuessed, chosen, {
          path: "/",
          sameSite: "lax",
          secure: appConfig.authCookieSecure,
          httpOnly: false,
        });
      }
      return res;
    }
  }

  if (!isPublic(pathname) && !signedIn) {
    const url = req.nextUrl.clone();
    url.pathname = `/${locale ?? "en"}${BASE_PATH}/login`;
    url.search = "";
    const returnTo = `${pathname}${search}`;
    if (returnTo !== "/") url.searchParams.set("returnTo", returnTo);
    return withCsp(NextResponse.redirect(url), csp);
  }

  if (signedIn && withoutPrefix(pathname) === "/login") {
    const url = req.nextUrl.clone();
    url.pathname = `/${locale ?? "en"}${BASE_PATH}`;
    url.search = "";
    return withCsp(NextResponse.redirect(url), csp);
  }

  return withCsp(withLocale(req, ab(req)), csp);
}

// Assigns a sticky locale on first visit — the same shape as the A/B cookie.
//
// `negotiate` reads Accept-Language, which is a *suggestion*. On the public site
// that distinction matters enormously: a language redirect can bury the other
// locales for a crawler arriving from a US IP, which is why the conductor never
// auto-switches. Here there is no crawler and no URL to redirect — the header is
// simply the best first guess for a signed-in human, and the cookie makes their
// correction stick.
//
// Deliberately never overwritten once set. A reader who chose English on a
// Russian-configured laptop must not be flipped back on the next navigation.
function withLocale(req: NextRequest, res: NextResponse): NextResponse {
  // The URL is authoritative now; this only remembers it, so an unprefixed
  // /cabinet entry later resolves to the language the reader was last reading
  // rather than re-guessing from a header they never set.
  const fromUrl = localeOf(req.nextUrl.pathname);
  if (!fromUrl && req.cookies.get(COOKIES.locale)?.value) return res;
  res.cookies.set(COOKIES.locale, fromUrl ?? negotiate(req.headers.get("accept-language")), {
    path: "/",
    sameSite: "lax",
    secure: appConfig.authCookieSecure,
    // Readable by the client switcher. It carries no authority, so HttpOnly
    // would buy nothing and cost the switcher a round trip.
    httpOnly: false,
    maxAge: 60 * 60 * 24 * 365,
  });
  return res;
}

function withCsp(res: NextResponse, csp: string): NextResponse {
  res.headers.set(CSP_HEADER, csp);
  return res;
}

export const config = {
  // "/" is listed explicitly: the negative lookahead alone never matches the bare
  // index, so an unauthenticated visitor reached the (app) shell at the zone root
  // while every sub-route correctly bounced to /login. Gate the index too.
  // `mfe/` is excluded like `_next/static`: the public/mfe/* element-remote bundles
  // are static assets the conductor injects even for anonymous visitors (the chip
  // renders its own signed-out CTA), so they must be reachable without a session.
  //
  // Each name is listed twice, bare and `cabinet/`-prefixed, because this zone no
  // longer has a `basePath` for Next to strip before matching — the real paths are
  // `/cabinet/_next/static/…`, `/cabinet/api/…`, `/cabinet/mfe/…`. The bare forms
  // stay for a direct (dev) run of the zone. `isNonPagePath` is the same rule in
  // code and is what the tests pin; keep the two in step.
  matcher: [
    "/",
    "/((?!(?:cabinet/)?(?:api|_next/static|_next/image|favicon.ico|mfe/)).*)",
  ],
};
