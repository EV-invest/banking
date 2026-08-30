"use client";

import { useEffect, useRef } from "react";

import { DEFAULT_LOCALE, isLocale, type Locale } from "@evinvest/i18n";
import { useLocale } from "@evinvest/i18n/react";

import { profileResource, saveProfile } from "@/entities/user/model/profile-resource";
import { relocalise } from "@/shared/config/base-path";
import { readLocaleCookie, writeLocaleCookie } from "@/shared/lib/locale-cookie";
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
// THE ASYMMETRY THIS IS BUILT AROUND, because getting it wrong quietly destroys data:
// English is the *unprefixed* locale, so `/` is simultaneously "English" and "no
// language expressed". Almost every reader arrives there — it is what a search result,
// a bookmark and every external link point at, and the public site deliberately never
// auto-switches away from it. Treating that as a choice means a reader who picked
// Deutsch in settings has `language: "en"` written over it the next time they open the
// landing and click the account chip, which is most days. So a locale is only ever
// taken as a *preference* when it could not have arrived by default:
//
//   • Accept-Language guesses are never preferences. The proxy marks them (it knows,
//     and by the time a page renders the evidence is gone). If the account already has
//     a stored language it wins outright — the reader who chose Deutsch on a laptop
//     opens the cabinet in German on a phone rather than in the phone's OS language.
//     If it does not, nothing is stored: a guess must not become the answer to the
//     question it was guessing at.
//
//   • `en` is never written from a URL, for the reason above. It is a real choice only
//     when made in settings, which writes it directly and does not need this component.
//
//   • Any of the four prefixed locales IS a signal. `/ru`, `/vi`, `/fr`, `/de` cannot
//     be reached by accident, so arriving in the cabinet from one is the reader telling
//     us their language, and it becomes the stored one. That is what makes the language
//     follow someone in from the landing without them ever opening settings.
export function LocaleSync() {
  const locale = useLocale();
  const { data: profile } = useResource(profileResource);
  // One attempt per (locale, stored) pair, so a background revalidation of the profile
  // does not re-run the write on every refresh.
  const attempted = useRef<string | null>(null);

  useEffect(() => {
    if (!profile) return;
    const stored = profile.language ?? "";
    const key = `${locale}:${stored}`;
    if (attempted.current === key) return;
    attempted.current = key;

    // Cleared unconditionally: the mark describes the entry that minted it, and this is
    // the page that entry led to. `guessed` is only true when it names THIS locale, so
    // a mark left behind by an earlier redirect cannot speak for a URL it never saw.
    const guessed = takeGuessMarker() === locale;
    if (guessed) {
      if (isLocale(stored) && stored !== locale) {
        writeLocaleCookie(stored);
        // Hard navigation: the locale is a root layout segment, so the catalogue is
        // chosen server-side and `router.replace` would re-render the same one.
        // `location.replace` rather than an assignment, so the guessed URL is not a
        // back-button stop the reader has to click past twice.
        window.location.replace(relocalise(stored, window.location));
      }
      // Either way the guess itself is not evidence of anything, so nothing is stored.
      return;
    }

    if (locale === DEFAULT_LOCALE) return;
    if (stored === locale) return;
    // A reader who stores something more specific than a routable locale (`en-GB`,
    // `pt-BR`) has a real preference this UI cannot express; narrowing it to the bare
    // language would be a downgrade, not a sync.
    if (stored !== "" && !isLocale(stored)) return;
    // The cookie is written from the URL by the proxy on every request, so the two agree
    // on any settled page. They disagree in exactly one window: the settings switcher
    // writes the cookie and *then* navigates, and the page keeps running until unload —
    // long enough for `saveProfile` to publish, re-render this effect with the new
    // stored language beside the old URL, and helpfully write the old language back over
    // the choice the reader just made.
    if (readLocaleCookie() !== locale) return;

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

/**
 * Reads the proxy's "this locale came from Accept-Language" mark and clears it,
 * returning the locale it was minted for.
 *
 * The name is derived from the protocol for the same reason as the locale cookie
 * itself — see `shared/lib/locale-cookie.ts`. Expired by setting `max-age=0` on the
 * exact same path the proxy wrote it with; anything else leaves the original in place.
 */
function takeGuessMarker(): Locale | null {
  const name = (location.protocol === "https:" ? "__Host-" : "") + "ev_locale_guessed";
  let value: string | null = null;
  for (const part of document.cookie.split(";")) {
    const [k, ...rest] = part.trim().split("=");
    if (k === name) value = decodeURIComponent(rest.join("="));
  }
  if (value !== null) {
    document.cookie = `${name}=;path=/;max-age=0${location.protocol === "https:" ? ";secure" : ""}`;
  }
  return isLocale(value) ? value : null;
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
