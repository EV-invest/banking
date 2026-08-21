import type { Locale } from "@evinvest/i18n";

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

/** @deprecated Ambiguous now that pages and assets diverge — say which you mean. */
export const withBasePath = apiPath;
