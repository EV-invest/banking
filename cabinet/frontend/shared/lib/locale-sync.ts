import { DEFAULT_LOCALE, isLocale, type Locale } from "@evinvest/i18n";

// What a URL proves about the language a signed-in reader wants — the decision the
// `LocaleSync` component (application/layout) carries out on every mount. Kept free of
// React and the DOM so the rule can be pinned down by tests; the component only
// gathers the four inputs and acts on the verdict.

export type LocaleSyncInput = {
  /** The locale of the page the reader is on. */
  locale: Locale;
  /** `profile.language` as the API reported it — `""` when the account has none. */
  stored: string;
  /** The proxy marked THIS locale as derived from Accept-Language, not from the reader. */
  guessed: boolean;
  /** The `ev_locale` cookie as the browser currently holds it. */
  cookie: Locale | null;
};

export type LocaleSyncDecision =
  | { kind: "none" }
  /** The URL was a guess and the account knows better: navigate to the stored one. */
  | { kind: "adopt-stored"; to: Locale }
  /** The account has no language yet and the URL is a deliberate one: seed it. */
  | { kind: "store"; language: Locale };

/**
 * The rule, in order of precedence. It used to be the other way round — any of the
 * four prefixed locales was read as "the most recent thing the reader did" and written
 * to the account — and a shared `/de/cabinet/profile` link then silently rewrote an
 * account set to Vietnamese on every device at once (#347). Settings promises "saved
 * to your account, so every device follows"; a preference that follows the reader
 * cannot also follow the link they clicked.
 *
 *   1. A guessed locale is never evidence. If the account has a routable language,
 *      it wins and the page is re-entered under it; otherwise nothing happens.
 *   2. A stored language is only ever changed from settings. Whatever the URL says —
 *      the same locale, a different one, or one the account's more specific `en-GB`
 *      cannot be reduced to — a non-empty `stored` is left alone.
 *   3. `en` is the unprefixed locale and so also "no language expressed"; it is
 *      never written from a URL.
 *   4. The cookie is written from the URL by the proxy on every request, so the two
 *      agree on any settled page. The one window where they disagree is the settings
 *      switcher writing the cookie and then navigating; a decision taken inside that
 *      window is discarded rather than raced.
 *   5. What remains is an account with no language yet, reached through one of the
 *      four prefixed locales on purpose: the one URL-driven write left. There is no
 *      choice to destroy, and it lets the language follow a new reader in from the
 *      landing without them opening settings.
 */
export function decideLocaleSync(input: LocaleSyncInput): LocaleSyncDecision {
  const { locale, stored, guessed, cookie } = input;
  if (guessed) {
    if (isLocale(stored) && stored !== locale) return { kind: "adopt-stored", to: stored };
    return { kind: "none" };
  }
  if (stored !== "") return { kind: "none" };
  if (locale === DEFAULT_LOCALE) return { kind: "none" };
  if (cookie !== locale) return { kind: "none" };
  return { kind: "store", language: locale };
}
