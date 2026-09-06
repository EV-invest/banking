// The auth cookie names this zone READS — they are minted by the shell-owned auth
// surface (concierge, behind the conductor's /api/auth rewrites), never set here.
// The proxy middleware gates on `session`; `csrf-client.ts` reads `csrf` for the
// double-submit header.

import { config } from "../../config.ts";

// `__Host-` cookies require the Secure attribute, which browsers reject over plain
// http://localhost. So in dev (no Secure) the shell drops the prefix; in production
// the cookies are `__Host-`-prefixed and Secure. Toggle explicitly with
// AUTH_COOKIE_SECURE, else infer from NODE_ENV — must match the shell's setting.
const SECURE = config.authCookieSecure;
const PREFIX = SECURE ? "__Host-" : "";

export const COOKIES = {
  session: `${PREFIX}ev_session`,
  csrf: `${PREFIX}ev_csrf`,
  // The one cookie this zone MINTS rather than reads, so the header comment
  // above does not cover it. It carries the reader's locale, because the cabinet
  // — unlike the public site — has no reason to put the locale in the URL: it is
  // entirely behind auth, so there is no crawler to serve alternates to and no
  // indexed URL to keep stable. See the i18n section of README.md.
  locale: `${PREFIX}ev_locale`,
  // Set alongside a redirect whose locale came from Accept-Language rather than
  // from the reader — see the guess branch in `proxy.ts`. Read and cleared by
  // `LocaleSync`, which uses it to decide whether the stored profile language
  // should override the URL. Session-scoped and carries no authority.
  localeGuessed: `${PREFIX}ev_locale_guessed`,
} as const;
