// OAuth client configuration. The cabinet is the OAuth confidential client; the
// Google client *secret* lives in the hub auth task, not here — the BFF only needs
// the public client id and the callback URL to build the authorize redirect.

export const GOOGLE_CLIENT_ID = process.env.GOOGLE_CLIENT_ID ?? "";
export const AUTH_REDIRECT_URI = process.env.AUTH_REDIRECT_URI ?? "http://localhost:3000/api/auth/callback";

/** Whether the OAuth login flow is wired (mirrors the hub's no-op-until-configured posture). */
export function authConfigured(): boolean {
  return GOOGLE_CLIENT_ID.length > 0;
}
