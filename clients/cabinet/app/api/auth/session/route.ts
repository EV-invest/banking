import { type NextRequest, NextResponse } from "next/server";

import { ensureFresh, readSessionId } from "@/shared/auth/session";

// Who-am-I for the browser: returns the signed-in user (refreshing the hub access
// token transparently if it expired), or `{ authenticated: false }`. Never returns
// any token to the client.
export async function GET(req: NextRequest) {
  const fresh = await ensureFresh(readSessionId(req));
  if (!fresh) {
    return NextResponse.json({ authenticated: false });
  }
  return NextResponse.json({ authenticated: true, user: fresh.user });
}
