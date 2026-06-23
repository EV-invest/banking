import { type NextRequest, NextResponse } from "next/server";

import { putSession, setSessionCookies } from "@/entities/session/model/session";
import { AUTH_REDIRECT_URI } from "@/features/auth/config";
import { safeReturnTo } from "@/features/auth/lib/oauth";
import { clearTxCookie, takeTx } from "@/features/auth/model/oauth-tx";
import { exchange } from "@/shared/api/auth";

// OAuth callback: validate the state against the stored transaction, exchange the
// code for the hub's tokens (server-to-server), open a session, and redirect home.
export async function GET(req: NextRequest) {
  const params = req.nextUrl.searchParams;
  const origin = req.nextUrl.origin;
  const fail = (reason: string) => {
    const res = NextResponse.redirect(new URL(`/login?error=${reason}`, origin));
    clearTxCookie(res);
    return res;
  };

  if (params.get("error")) return fail("denied");

  const code = params.get("code");
  const state = params.get("state");
  // The transaction is keyed by the `ev_oauth_tx` cookie, which is HttpOnly and
  // (in prod) `__Host-`-prefixed — so only the browser that *started* the flow
  // holds it, binding this callback to that user-agent. The `state` must then match
  // the stored tx (defeats response tampering/replay). Together this is the
  // standard OAuth login-CSRF / session-fixation mitigation.
  const tx = takeTx(req);
  if (!code || !state || !tx || tx.state !== state) return fail("invalid");

  try {
    // Device metadata for the "sessions & devices" surface — stored on the new
    // refresh-token family at the hub. Best-effort; behind a proxy use the forwarded IP.
    const userAgent = req.headers.get("user-agent") ?? "";
    const ip = (req.headers.get("x-forwarded-for") ?? "").split(",")[0]?.trim() || (req.headers.get("x-real-ip") ?? "");
    const tokens = await exchange({ auth_code: code, code_verifier: tx.codeVerifier, redirect_uri: AUTH_REDIRECT_URI, nonce: tx.nonce, user_agent: userAgent, ip });
    const { id, csrfToken, maxAge } = putSession(tokens);
    const res = NextResponse.redirect(new URL(safeReturnTo(tx.returnTo), origin));
    setSessionCookies(res, id, csrfToken, maxAge);
    clearTxCookie(res);
    return res;
  } catch (e) {
    // Surface the hub's gRPC status (e.g. NotConfigured / google token endpoint
    // returned 400 / nonce mismatch) server-side — the user only sees `?error=exchange`.
    console.error("[auth/callback] token exchange failed:", (e as { code?: number; details?: string; message?: string }).details ?? (e as Error).message ?? e);
    return fail("exchange");
  }
}
