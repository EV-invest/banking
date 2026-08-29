"use client";

import { useEffect, useRef } from "react";

import { isLocale, type Locale } from "@evinvest/i18n";
import { useLocale } from "@evinvest/i18n/react";

import { profileResource, saveProfile } from "@/entities/user/model/profile-resource";
import { cabinetPath, zonePathname } from "@/shared/config/base-path";
import { writeLocaleCookie } from "@/shared/lib/locale-cookie";
import { useResource } from "@/shared/lib/resource";
import type { UpdateProfileRequest, UserProfile } from "@/shared/contracts";

// Keeps three representations of "what language does this reader want" from drifting:
// the URL they are on, the `ev_locale` cookie the proxy resolves unprefixed entries
// with, and `profile.language` — the only one of the three that survives a new device.
//
// Renders nothing and mounts once, in the signed-in `(app)` layout: the profile only
// exists for a signed-in reader, and `(auth)` deliberately has no session to read one
// with.
//
// The rule is that the most recent thing the reader actually *did* wins, and there are
// exactly two cases:
//
//   • The proxy had to guess. It reaches an unprefixed `/cabinet/*` with no cookie —
//     a first visit on this device — and falls back to Accept-Language, a header the
//     reader never set. It marks that guess (COOKIES.localeGuessed). If the account
//     has a stored language, it beats the guess: adopt it, and the reader who chose
//     Deutsch on their laptop opens the cabinet in German on their phone instead of
//     in whatever their phone's OS language happens to be.
//
//   • Everything else. The locale in the URL came from somewhere real — the language
//     they were reading on the public site (site_conductor mirrors it into the cookie,
//     and the account chip links straight into it), the settings switcher, or a link
//     they followed. That is a choice, so it becomes the stored one.
//
// The second case is what makes the language "follow" a reader from the landing without
// them ever opening settings, and it is deliberately a write on arrival rather than a
// prompt: the alternative is a stored preference that silently disagrees with every page
// they have seen for months.
export function LocaleSync() {
  const locale = useLocale();
  const { data: profile } = useResource(profileResource);
  // One attempt per (locale, stored) pair. Without this a save that keeps failing —
  // an expired session, a validation rule the stored profile already violates — would
  // be retried on every render of every page for as long as the tab is open.
  const attempted = useRef<string | null>(null);

  useEffect(() => {
    if (!profile) return;
    const stored = profile.language ?? "";
    const key = `${locale}:${stored}`;
    if (attempted.current === key) return;
    attempted.current = key;

    if (takeGuessMarker() && isLocale(stored) && stored !== locale) {
      writeLocaleCookie(stored);
      // A hard navigation: the locale is a root layout segment, so the catalogue is
      // chosen server-side and `router.replace` would re-render the same one.
      // `replace` semantics via location.replace so the guessed URL does not become a
      // back-button stop the reader has to click past twice.
      window.location.replace(localeUrl(stored));
      return;
    }

    // Already agreed, or the reader stores something more specific than a routable
    // locale (`en-GB`, `pt-BR`): a regional code is a real preference that this UI
    // cannot express, so narrowing it to the bare language would be a downgrade, not
    // a sync. Only overwrite an empty value or another routable locale.
    if (stored === locale) return;
    if (stored !== "" && !isLocale(stored)) return;

    // Best-effort, and deliberately not retried within the page. The cookie already
    // carries this locale, so nothing the reader can see is waiting on it — only the
    // cross-device copy is missed, and the next full page load (every cross-zone
    // navigation is one) tries again. Retrying in place would instead mean a profile
    // the server will never accept — a legacy field that no longer validates — being
    // re-POSTed on every 60s revalidation for as long as the tab is open.
    void saveProfile(languageOnly(profile, locale)).catch(() => {});
  }, [profile, locale]);

  return null;
}

/** The current page, in `locale`, query string and all. */
function localeUrl(locale: Locale): string {
  return cabinetPath(locale, zonePathname(window.location.pathname)) + window.location.search;
}

/**
 * Reads the proxy's "this locale was guessed from Accept-Language" mark and clears it,
 * so the adoption below happens once per guess rather than on every page of the visit.
 *
 * The name is derived from the protocol for the same reason as the locale cookie
 * itself — see `shared/lib/locale-cookie.ts`. Expired by setting `max-age=0` on the
 * exact same path the proxy wrote it with; anything else leaves the original in place.
 */
function takeGuessMarker(): boolean {
  const name = (location.protocol === "https:" ? "__Host-" : "") + "ev_locale_guessed";
  const present = document.cookie
    .split(";")
    .some((part) => part.trim().split("=")[0] === name);
  if (present) {
    document.cookie = `${name}=;path=/;max-age=0${location.protocol === "https:" ? ";secure" : ""}`;
  }
  return present;
}

/**
 * `language` changed, every other field as the server last reported it.
 *
 * UpdateProfile is full-replace — an omitted field is a cleared field — so a partial
 * body here would wipe the reader's name and address as a side effect of them having
 * browsed the site in French.
 */
function languageOnly(profile: UserProfile, language: Locale): UpdateProfileRequest {
  return {
    legal_name: profile.legal_name ?? "",
    preferred_name: profile.preferred_name ?? "",
    phone: profile.phone ?? "",
    date_of_birth: profile.date_of_birth ?? "",
    nationality: profile.nationality ?? "",
    tax_residence: profile.tax_residence ?? "",
    residential_address: profile.residential_address ?? "",
    language,
    base_currency: profile.base_currency ?? "",
    timezone: profile.timezone ?? "",
  };
}
