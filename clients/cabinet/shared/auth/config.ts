// Auth/BFF configuration. The cabinet is the OAuth confidential client; the Google
// client *secret* lives in the hub auth task, not here — the BFF only needs the
// public client id and the callback URL to build the authorize redirect.

export const GOOGLE_CLIENT_ID = process.env.GOOGLE_CLIENT_ID ?? "";
export const AUTH_REDIRECT_URI = process.env.AUTH_REDIRECT_URI ?? "http://localhost:3000/api/auth/callback";

/** Whether the OAuth login flow is wired (mirrors the hub's no-op-until-configured posture). */
export function authConfigured(): boolean {
  return GOOGLE_CLIENT_ID.length > 0;
}

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
