import { type NextRequest, NextResponse } from "next/server";

import { readSessionId, refreshTokenFor } from "@/entities/session/model/session";
import { verifyCsrf } from "@/features/auth/lib/csrf";
import { listSessions, revokeSession } from "@/shared/api/auth";
import { errorDetail, httpStatusFor } from "@/shared/api/wallet";

// The caller's active sessions (refresh-token families at the hub). Identity is proven by
// the session's server-side refresh token — never the access token, and the refresh token
// itself is never sent to the browser. The current device is flagged by the hub.
export async function GET(req: NextRequest) {
  const refresh = refreshTokenFor(readSessionId(req));
  if (!refresh) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  try {
    return NextResponse.json(await listSessions(refresh));
  } catch (err) {
    return NextResponse.json({ error: errorDetail(err) }, { status: httpStatusFor(err) });
  }
}

// Revoke one session by id. The UI only offers this for non-current devices; revoking the
// current family would simply self-heal (its refresh fails → /api/auth/session clears the cookie).
export async function DELETE(req: NextRequest) {
  if (!verifyCsrf(req)) return NextResponse.json({ error: "csrf" }, { status: 403 });
  const refresh = refreshTokenFor(readSessionId(req));
  if (!refresh) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  const body = (await req.json().catch(() => ({}))) as { session_id?: string };
  const sessionId = typeof body.session_id === "string" ? body.session_id : "";
  if (!sessionId) return NextResponse.json({ error: "session_id required" }, { status: 400 });
  try {
    await revokeSession(refresh, sessionId);
    return NextResponse.json({ ok: true });
  } catch (err) {
    return NextResponse.json({ error: errorDetail(err) }, { status: httpStatusFor(err) });
  }
}
