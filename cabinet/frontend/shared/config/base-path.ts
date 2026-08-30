import { isLocale, type Locale } from "@evinvest/i18n";

// The zone mount prefix. The cabinet is a Next.js multi-zone served under the
// conductor's domain at /cabinet.
//
// Two different things now live under this prefix and they no longer have the
// same shape:
//
//   assets + API   /cabinet/_next/*, /cabinet/api/*      — no locale, ever
//   pages          /{locale}/cabinet/wallet              — locale first
//
// `basePath` used to make the first case automatic and is gone (it would have
// prepended itself to the second, turning /ru/cabinet/wallet into
// /cabinet/ru/cabinet/wallet — see docs/i18n-cabinet-routing-spike.md).
// `assetPrefix` covers `/_next/*`; everything else is explicit through the two
// helpers below, and which one you want is decided by whether a *reader* sees it.
export const BASE_PATH = "/cabinet";

/**
 * A BFF route. Locale-free on purpose: the API answers in data, not prose, and
 * the one endpoint whose payload is language-dependent takes the locale as a
 * parameter rather than living at a different URL per language.
 */
export const apiPath = (path: `/${string}`): string => `${BASE_PATH}${path}`;

/**
 * A page URL the reader can end up looking at. Every in-cabinet link goes through
 * here — with `basePath` gone, a bare `<Link href="/wallet">` now points at the
 * conductor's origin root instead of the zone, which fails as a 404 on a page
 * that has nothing to do with the cabinet.
 */
export const cabinetPath = (locale: Locale, path: `/${string}`): string =>
  path === "/" ? `/${locale}${BASE_PATH}` : `/${locale}${BASE_PATH}${path}`;

/**
 * The inverse of {@link cabinetPath}: a real request path back to the zone-relative
 * one the app reasons in.
 *
 * `usePathname()` used to hand back a basePath-stripped path for free, so the nav
 * could compare it against a plain `/wallet`. With `basePath` gone it returns the
 * whole thing — `/ru/cabinet/wallet` — and every such comparison silently stops
 * matching. Reading a path is therefore as much of a rule as writing one, and it
 * belongs next to its twin rather than inlined at each caller.
 *
 * Unrecognised input degrades instead of throwing: a missing or unknown locale segment
 * is left alone, and a path outside the zone comes back unchanged.
 */
export const zonePathname = (pathname: string): `/${string}` => {
  // The locale segment is matched against LOCALES directly rather than through the i18n
  // package's `splitLocalePath`, which leaves an explicit default-locale prefix in place
  // — `/en/cabinet/wallet` comes back whole from it, so every English URL (the majority)
  // would have gone unstripped.
  const [, first, ...rest] = pathname.split("/");
  const path = isLocale(first) ? `/${rest.join("/")}` : pathname;
  if (path === BASE_PATH) return "/";
  // Compared with the trailing slash so a sibling like `/cabinets` is not mistaken for
  // this zone and half-eaten.
  return (path.startsWith(`${BASE_PATH}/`) ? path.slice(BASE_PATH.length) : path) as `/${string}`;
};

/**
 * Every request shape under this prefix that is NOT a page: Next's build output,
 * the BFF, and the element-remote bundles the conductor injects for anonymous
 * visitors. None of them may be locale-redirected or gated behind the session.
 *
 * The optional zone prefix is the entire point. `basePath` used to strip
 * `/cabinet` before the proxy matcher ran, so naming these anchored at the
 * leading `/` was right. With `basePath` gone (see the note at the top of this
 * file) the proxy sees the real `/cabinet/_next/...`, an anchored test matches
 * nothing, and every stylesheet, script, font and API call is treated as a
 * locale-less page: 307 to `/{locale}/cabinet/…`, then bounced to `/login`,
 * which answers `200 text/html`. The browser refuses each one for its MIME type,
 * so the cabinet renders unstyled and never hydrates. That shipped in v0.2.57.
 *
 * The set of names is deliberately unchanged from the matcher's original list —
 * widening it (say, to all of `_next/`) would quietly ungate paths this zone has
 * never exposed.
 */
const NON_PAGE = new RegExp(
  `^(?:${BASE_PATH})?/(?:_next/static/|_next/image|api/|mfe/|favicon\\.ico$)`
);

/** @see NON_PAGE — true when `pathname` is an asset, a BFF call or an MFE bundle. */
export const isNonPagePath = (pathname: string): boolean => NON_PAGE.test(pathname);

/**
 * The same cabinet page, in another locale — path, query and hash intact.
 *
 * Two callers relocalise the current page (the settings language switcher and
 * `LocaleSync`) and they must agree: one of them dropping the query string is a
 * bug that only appears the first time a cabinet page takes a parameter. Takes
 * the URL parts rather than reading `window` so it stays pure and testable
 * alongside its siblings here.
 */
export const relocalise = (
  locale: Locale,
  url: { pathname: string; search: string; hash: string },
): string => cabinetPath(locale, zonePathname(url.pathname)) + url.search + url.hash;

/** @deprecated Ambiguous now that pages and assets diverge — say which you mean. */
export const withBasePath = apiPath;
