import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { verifyCsrf } from "@/features/auth/lib/csrf";
import { cancelWithdrawal, errorDetail, httpStatusFor } from "@/shared/api/wallet";

// Cancel one of the signed-in user's still-queued withdrawals — CSRF-checked (it moves
// money back). The hub enforces ownership and that the withdrawal is still queued.
export async function POST(req: NextRequest) {
  if (!verifyCsrf(req)) {
    return NextResponse.json({ error: "csrf" }, { status: 403 });
  }
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  const body = (await req.json().catch(() => null)) as { withdrawal_id?: string } | null;
  if (!body?.withdrawal_id) {
    return NextResponse.json({ error: "withdrawal_id is required" }, { status: 400 });
  }
  try {
    return NextResponse.json(await cancelWithdrawal(token, body.withdrawal_id));
  } catch (err) {
    return NextResponse.json({ error: errorDetail(err) }, { status: httpStatusFor(err) });
  }
}
