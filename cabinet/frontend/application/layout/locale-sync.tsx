"use client";

import { useEffect, useRef } from "react";

import { isLocale, type Locale } from "@evinvest/i18n";
import { useLocale } from "@evinvest/i18n/react";

import { profileResource, saveProfile } from "@/entities/user/model/profile-resource";
import { relocalise } from "@/shared/config/base-path";
import { readLocaleCookie, writeLocaleCookie } from "@/shared/lib/locale-cookie";
import { useResource } from "@/shared/lib/resource";
import type { UpdateProfileRequest, UserProfile } from "@/shared/contracts";

import { decideLocaleSync } from "./locale-sync-policy";

// Keeps three representations of "what language does this reader want" from drifting:
// the URL they are on, the `ev_locale` cookie the proxy resolves unprefixed entries
// with, and `profile.language` — the only one of the three that survives a new device.
//
// Renders nothing and mounts once, in the signed-in `(app)` layout: the profile only
// exists for a signed-in reader, and `(auth)` deliberately has no session to read one
// with.
//
// THE RULE: settings is the only thing that changes a stored language. A URL never
// overwrites one (#347). This used to be the other way round — any of the four
// prefixed locales was read as "the most recent thing the reader did" and written to
// the account — and the consequence was that a `/de/cabinet/profile` link someone
// shared, or a tester's GET, silently rewrote an account set to Vietnamese, on every
// device at once. Settings promises "saved to your account, so every device follows";
// a preference that follows the reader cannot also follow the link they clicked.
//
// What this component still does:
//
//   • Reads. When the proxy guessed the locale from Accept-Language (it marks those —
//     by the time a page renders the evidence is gone) and the account has a routable
//     language of its own, that one wins: the reader who chose Deutsch on a laptop
//     opens the cabinet in German on a phone, not in the phone's OS language. A guess
//     is never stored, whether or not the account has a language.
//
//   • One bootstrap write. An account with no language at all (`""` from the API),
//     entered through a prefixed locale that was not a guess, has that locale stored
//     — there is no choice to destroy, and it lets the language follow a new reader
//     in from the landing without them opening settings. `en` is the unprefixed
//     locale, so `/` is also "nothing expressed" and is never written from a URL.
//
// The verdict itself is `decideLocaleSync` in `./locale-sync-policy.ts`, pure and
// tested; this file only gathers its inputs and carries it out.
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
    const decision = decideLocaleSync({
      locale,
      stored,
      guessed,
      cookie: readLocaleCookie(),
    });

    if (decision.kind === "adopt-stored") {
      writeLocaleCookie(decision.to);
      // Hard navigation: the locale is a root layout segment, so the catalogue is
      // chosen server-side and `router.replace` would re-render the same one.
      // `location.replace` rather than an assignment, so the guessed URL is not a
      // back-button stop the reader has to click past twice.
      window.location.replace(relocalise(decision.to, window.location));
      return;
    }
    if (decision.kind !== "store") return;

    // Best-effort, and deliberately not retried within the page. The cookie already
    // carries this locale, so nothing the reader can see is waiting on it — only the
    // cross-device copy is missed, and the next full page load (every cross-zone
    // navigation is one) tries again. Retrying in place would instead mean a profile
    // the server will never accept — a legacy field that no longer validates — being
    // re-POSTed on every 60s revalidation for as long as the tab is open.
    void saveProfile(languageOnly(profile, decision.language)).catch(() => {});
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
