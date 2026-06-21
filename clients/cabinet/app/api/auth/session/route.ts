import { type NextRequest, NextResponse } from "next/server";

import { clearSessionCookies, ensureFresh, readSessionId } from "@/entities/session/model/session";

// Who-am-I for the browser: returns the signed-in user (refreshing the hub access
// token transparently if it expired), or `{ authenticated: false }`. Never returns
// any token to the client.
export async function GET(req: NextRequest) {
  const fresh = await ensureFresh(readSessionId(req));
  if (!fresh) {
    // The session is gone/expired but the browser may still hold the opaque `ev_session`
    // cookie (e.g. after a dev restart). Clear it here so the proxy stops treating the
    // request as signed-in — otherwise the client redirect to /login and the proxy's
    // signed-in→/ bounce ping-pong forever.
    const res = NextResponse.json({ authenticated: false });
    clearSessionCookies(res);
    return res;
  }
  return NextResponse.json({ authenticated: true, user: fresh.user });
}
