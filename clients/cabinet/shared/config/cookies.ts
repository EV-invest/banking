// App-wide cookie configuration: the names and base options for every cookie the
// BFF sets. Lives in shared so both the session entity and the auth feature bind
// to the same cookie identity without cross-importing each other.

// `__Host-` cookies require the Secure attribute, which browsers reject over plain
// http://localhost. So in dev (no Secure) we drop the prefix; in production the
// cookies are `__Host-`-prefixed and Secure. Toggle explicitly with
// AUTH_COOKIE_SECURE, else infer from NODE_ENV.
const SECURE = process.env.AUTH_COOKIE_SECURE ? process.env.AUTH_COOKIE_SECURE === "true" : process.env.NODE_ENV === "production";
const PREFIX = SECURE ? "__Host-" : "";

export const COOKIES = {
  session: `${PREFIX}ev_session`,
  csrf: `${PREFIX}ev_csrf`,
  oauthTx: `${PREFIX}ev_oauth_tx`,
} as const;

/** Base cookie options for the server-side, HttpOnly cookies. */
export const cookieBase = {
  httpOnly: true,
  secure: SECURE,
  sameSite: "lax" as const,
  path: "/",
};
