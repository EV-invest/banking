import { cookies } from "next/headers";
import { isLocale, type Locale } from "@evinvest/i18n";

import { COOKIES } from "./cookies";

/**
 * The reader's locale, from the cookie the proxy minted.
 *
 * No URL segment, unlike the public site. The cabinet sits entirely behind auth:
 * there is no crawler to serve `hreflang` alternates to, and no indexed URL whose
 * shape has to stay stable — so a locale prefix would buy nothing and would cost
 * a restructuring of all 19 routes around a `[locale]` segment.
 *
 * Falls back to English rather than throwing. The cookie is absent on the very
 * first request (the proxy sets it on the way out), and a missing cookie has to
 * render a page, not a 500.
 *
 * Kept apart from ./i18n.ts on purpose: this needs the app config, via the
 * Secure-dependent cookie names, and importing it there made the catalogue audit
 * boot the whole env-validated config just to diff some JSON.
 */
export async function currentLocale(): Promise<Locale> {
  const value = (await cookies()).get(COOKIES.locale)?.value;
  return isLocale(value) ? value : "en";
}
