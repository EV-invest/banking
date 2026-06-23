// OAuth transaction store: the short-lived login handshake state (PKCE verifier,
// state, nonce, post-login target) that bridges the authorize redirect and the
// callback. Keyed by the HttpOnly `ev_oauth_tx` cookie so only the browser that
// *started* the flow can complete it. In-process Map — single-instance/dev only,
// same caveat as the session store.

import type { NextRequest, NextResponse } from "next/server";

import { COOKIES, cookieBase } from "@/shared/config/cookies";
import { randomId } from "@/shared/lib/crypto";

interface OAuthTx {
  state: string;
  nonce: string;
  codeVerifier: string;
  returnTo: string;
  createdAt: number;
}

const txns = new Map<string, OAuthTx>();
const OAUTH_TX_TTL_SECS = 600;

function nowSecs(): number {
  return Math.floor(Date.now() / 1000);
}

export function putTx(tx: Omit<OAuthTx, "createdAt">): string {
  const id = randomId();
  txns.set(id, { ...tx, createdAt: nowSecs() });
  return id;
}

/** Read + consume the transaction bound to this request's `ev_oauth_tx` cookie. */
export function takeTx(req: NextRequest): OAuthTx | null {
  const id = req.cookies.get(COOKIES.oauthTx)?.value;
  if (!id) return null;
  const tx = txns.get(id);
  txns.delete(id);
  if (!tx || nowSecs() - tx.createdAt > OAUTH_TX_TTL_SECS) return null;
  return tx;
}

export function setTxCookie(res: NextResponse, id: string): void {
  res.cookies.set(COOKIES.oauthTx, id, { ...cookieBase, maxAge: OAUTH_TX_TTL_SECS });
}

export function clearTxCookie(res: NextResponse): void {
  res.cookies.set(COOKIES.oauthTx, "", { ...cookieBase, maxAge: 0 });
}
