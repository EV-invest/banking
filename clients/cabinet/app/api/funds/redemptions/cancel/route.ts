import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { verifyCsrf } from "@/features/auth/lib/csrf";
import { cancelRedemption, errorDetail, httpStatusFor } from "@/shared/api/funds";

// Cancel one of the signed-in user's still-queued redemptions — CSRF-checked (it returns
// reserved units). The hub enforces ownership and that the redemption is still queued.
export async function POST(req: NextRequest) {
  if (!verifyCsrf(req)) {
    return NextResponse.json({ error: "csrf" }, { status: 403 });
  }
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  const body = (await req.json().catch(() => null)) as { redemption_id?: string } | null;
  if (!body?.redemption_id) {
    return NextResponse.json({ error: "redemption_id is required" }, { status: 400 });
  }
  try {
    return NextResponse.json(await cancelRedemption(token, body.redemption_id));
  } catch (err) {
    return NextResponse.json({ error: errorDetail(err) }, { status: httpStatusFor(err) });
  }
}
