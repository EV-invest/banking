// Server-side session store (the BFF token-handler pattern): the browser only ever
// holds an opaque session id cookie; the hub's JWTs live here, server-side, and are
// refreshed transparently. CSRF token is mirrored to a readable cookie for the
// double-submit check.
//
// Storage is an in-process Map — single-instance/dev only (mirrors the hub's
// in-process refresh store). PRODUCTION: back this with a session store
// (SESSION_REDIS_URL), distinct from the auth service's own refresh-rotation Redis.

import type { NextRequest, NextResponse } from "next/server";

import type { User } from "@/entities/user/@x/session";
import { refresh as grpcRefresh, type TokenResponse } from "@/shared/api/auth";
import { COOKIES, cookieBase } from "@/shared/config/cookies";
import { randomId } from "@/shared/lib/crypto";

interface Session {
  accessToken: string;
  accessExpiresAt: number;
  refreshToken: string;
  refreshExpiresAt: number;
  user: User;
  csrfToken: string;
}

const sessions = new Map<string, Session>();

function nowSecs(): number {
  return Math.floor(Date.now() / 1000);
}

function principal(tokens: TokenResponse): User {
  return { userId: tokens.user.user_id, email: tokens.user.email, status: tokens.user.status };
}

export function putSession(tokens: TokenResponse): { id: string; csrfToken: string; maxAge: number } {
  const id = randomId();
  const csrfToken = randomId();
  const refreshExpiresAt = Number(tokens.refresh_expires_at);
  sessions.set(id, {
    accessToken: tokens.access_token,
    accessExpiresAt: Number(tokens.access_expires_at),
    refreshToken: tokens.refresh_token,
    refreshExpiresAt,
    user: principal(tokens),
    csrfToken,
  });
  return { id, csrfToken, maxAge: Math.max(0, refreshExpiresAt - nowSecs()) };
}

export function setSessionCookies(res: NextResponse, id: string, csrfToken: string, maxAge: number): void {
  res.cookies.set(COOKIES.session, id, { ...cookieBase, maxAge });
  // The CSRF cookie is read by client JS for the double-submit header, so not HttpOnly.
  res.cookies.set(COOKIES.csrf, csrfToken, { ...cookieBase, httpOnly: false, maxAge });
}

export function clearSessionCookies(res: NextResponse): void {
  res.cookies.set(COOKIES.session, "", { ...cookieBase, maxAge: 0 });
  res.cookies.set(COOKIES.csrf, "", { ...cookieBase, httpOnly: false, maxAge: 0 });
}

export function readSessionId(req: NextRequest): string | undefined {
  return req.cookies.get(COOKIES.session)?.value;
}

/** The signed-in user for a session id, without refreshing (cheap guard read). */
export function currentUser(id: string | undefined): User | null {
  if (!id) return null;
  const session = sessions.get(id);
  if (!session || session.refreshExpiresAt <= nowSecs()) return null;
  return session.user;
}

/**
 * Ensure the session's access token is valid, rotating via the hub if it expired
 * (and the refresh token is still good). Returns the user + csrf token, or null if
 * the session is gone/expired (in which case it is dropped).
 */
export async function ensureFresh(id: string | undefined): Promise<{ user: User; csrfToken: string } | null> {
  if (!id) return null;
  const session = sessions.get(id);
  if (!session) return null;
  if (session.refreshExpiresAt <= nowSecs()) {
    sessions.delete(id);
    return null;
  }
  if (session.accessExpiresAt <= nowSecs() + 30) {
    try {
      const tokens = await grpcRefresh(session.refreshToken);
      session.accessToken = tokens.access_token;
      session.accessExpiresAt = Number(tokens.access_expires_at);
      session.refreshToken = tokens.refresh_token;
      session.refreshExpiresAt = Number(tokens.refresh_expires_at);
      session.user = principal(tokens);
    } catch {
      sessions.delete(id);
      return null;
    }
  }
  return { user: session.user, csrfToken: session.csrfToken };
}

/**
 * The fresh hub access token for a session — **server-only**, for BFF→hub gRPC calls
 * that act on the user's behalf. Rotates the token first (via {@link ensureFresh}),
 * so the returned token is valid; null if the session is gone/expired. Never expose
 * this to the browser.
 */
export async function accessTokenFor(id: string | undefined): Promise<string | null> {
  const fresh = await ensureFresh(id);
  if (!fresh || !id) return null;
  return sessions.get(id)?.accessToken ?? null;
}

/** Forget a session, returning its refresh token so the caller can revoke it at the hub. */
export function dropSession(id: string | undefined): string | null {
  if (!id) return null;
  const session = sessions.get(id);
  sessions.delete(id);
  return session?.refreshToken ?? null;
}
