import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { verifyCsrf } from "@/features/auth/lib/csrf";
import { errorDetail, httpStatusFor, listWithdrawals, requestWithdrawal } from "@/shared/api/wallet";

// The signed-in user's withdrawals, newest first.
export async function GET(req: NextRequest) {
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  try {
    return NextResponse.json(await listWithdrawals(token));
  } catch (err) {
    return NextResponse.json({ error: "withdrawals unavailable" }, { status: httpStatusFor(err) });
  }
}

// Open a withdrawal — CSRF-checked (it moves money). The hub reserves the funds and
// validates the address/amount/balance; its client-safe error detail is surfaced.
export async function POST(req: NextRequest) {
  if (!verifyCsrf(req)) {
    return NextResponse.json({ error: "csrf" }, { status: 403 });
  }
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  const body = (await req.json().catch(() => null)) as { network?: string; address?: string; amount?: string } | null;
  if (!body?.network || !body?.address || !body?.amount) {
    return NextResponse.json({ error: "network, address and amount are required" }, { status: 400 });
  }
  try {
    const withdrawal = await requestWithdrawal(token, { network: body.network, address: body.address, amount: body.amount });
    return NextResponse.json(withdrawal);
  } catch (err) {
    return NextResponse.json({ error: errorDetail(err) }, { status: httpStatusFor(err) });
  }
}
