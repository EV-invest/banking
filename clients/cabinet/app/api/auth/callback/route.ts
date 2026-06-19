import { type NextRequest, NextResponse } from "next/server";

import { AUTH_REDIRECT_URI, COOKIES } from "@/shared/auth/config";
import { safeReturnTo } from "@/shared/auth/oauth";
import { clearTxCookie, putSession, setSessionCookies, takeTx } from "@/shared/auth/session";
import { exchange } from "@/shared/bff/auth";

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
  const tx = takeTx(req.cookies.get(COOKIES.oauthTx)?.value);
  if (!code || !state || !tx || tx.state !== state) return fail("invalid");

  try {
    const tokens = await exchange({ auth_code: code, code_verifier: tx.codeVerifier, redirect_uri: AUTH_REDIRECT_URI, nonce: tx.nonce });
    const { id, csrfToken, maxAge } = putSession(tokens);
    const res = NextResponse.redirect(new URL(safeReturnTo(tx.returnTo), origin));
    setSessionCookies(res, id, csrfToken, maxAge);
    clearTxCookie(res);
    return res;
  } catch {
    return fail("exchange");
  }
}
