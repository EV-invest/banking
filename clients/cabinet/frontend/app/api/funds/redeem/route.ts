import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { verifyCsrf } from "@/features/auth/lib/csrf";
import { errorDetail, httpStatusFor, redeem } from "@/shared/api/funds";

// Redeem units of a fund back to cash (accept-and-queue) — CSRF-checked (it moves
// money). The hub reserves the units and prices the cash at settle; its client-safe
// error detail is surfaced.
export async function POST(req: NextRequest) {
  if (!verifyCsrf(req)) {
    return NextResponse.json({ error: "csrf" }, { status: 403 });
  }
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  const body = (await req.json().catch(() => null)) as { service?: string; units?: string } | null;
  if (!body?.service || !body?.units) {
    return NextResponse.json({ error: "service and units are required" }, { status: 400 });
  }
  try {
    const redemption = await redeem(token, { service: body.service, units: body.units });
    return NextResponse.json(redemption);
  } catch (err) {
    return NextResponse.json({ error: errorDetail(err) }, { status: httpStatusFor(err) });
  }
}
