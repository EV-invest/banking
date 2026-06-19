import { type NextRequest, NextResponse } from "next/server";

import { AUTH_REDIRECT_URI, GOOGLE_CLIENT_ID, authConfigured } from "@/features/auth/config";
import { authorizeUrl, newChallenge, safeReturnTo } from "@/features/auth/lib/oauth";
import { putTx, setTxCookie } from "@/features/auth/model/oauth-tx";

// Start of the OAuth login: mint PKCE/state/nonce, stash the transaction
// server-side, and redirect the browser to Google's consent screen.
export async function GET(req: NextRequest) {
  if (!authConfigured()) {
    return NextResponse.json({ error: "auth not configured" }, { status: 503 });
  }
  const returnTo = safeReturnTo(req.nextUrl.searchParams.get("returnTo"));
  const challenge = await newChallenge();
  const txId = putTx({ state: challenge.state, nonce: challenge.nonce, codeVerifier: challenge.codeVerifier, returnTo });

  const res = NextResponse.redirect(
    authorizeUrl({
      clientId: GOOGLE_CLIENT_ID,
      redirectUri: AUTH_REDIRECT_URI,
      state: challenge.state,
      nonce: challenge.nonce,
      codeChallenge: challenge.codeChallenge,
    }),
  );
  setTxCookie(res, txId);
  return res;
}
