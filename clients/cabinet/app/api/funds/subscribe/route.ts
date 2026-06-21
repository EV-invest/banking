import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { verifyCsrf } from "@/features/auth/lib/csrf";
import { errorDetail, httpStatusFor, subscribe } from "@/shared/api/funds";

// Subscribe free balance into a fund (mints units at the current NAV) — CSRF-checked
// (it moves money). The hub reserves the cash and prices the units; its client-safe
// error detail is surfaced.
export async function POST(req: NextRequest) {
  if (!verifyCsrf(req)) {
    return NextResponse.json({ error: "csrf" }, { status: 403 });
  }
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  const body = (await req.json().catch(() => null)) as { service?: string; amount?: string } | null;
  if (!body?.service || !body?.amount) {
    return NextResponse.json({ error: "service and amount are required" }, { status: 400 });
  }
  try {
    const subscription = await subscribe(token, { service: body.service, amount: body.amount });
    return NextResponse.json(subscription);
  } catch (err) {
    return NextResponse.json({ error: errorDetail(err) }, { status: httpStatusFor(err) });
  }
}
