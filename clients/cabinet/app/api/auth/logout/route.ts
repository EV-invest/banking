import { type NextRequest, NextResponse } from "next/server";

import { verifyCsrf } from "@/shared/auth/csrf";
import { clearSessionCookies, dropSession, readSessionId } from "@/shared/auth/session";
import { logout as grpcLogout } from "@/shared/bff/auth";

// Sign out: CSRF-checked, drops the server-side session and revokes the refresh
// family at the hub, then clears the browser cookies.
export async function POST(req: NextRequest) {
  if (!verifyCsrf(req)) {
    return NextResponse.json({ error: "csrf" }, { status: 403 });
  }
  const refreshToken = dropSession(readSessionId(req));
  if (refreshToken) {
    // Best-effort: the session is already gone locally; a hub blip must not block logout.
    try {
      await grpcLogout(refreshToken, false);
    } catch {
      /* ignore */
    }
  }
  const res = NextResponse.json({ ok: true });
  clearSessionCookies(res);
  return res;
}
