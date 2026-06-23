import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { getDepositAddress, httpStatusFor } from "@/shared/api/wallet";

// The signed-in user's deposit address on `?network=` (+ min confirmations).
export async function GET(req: NextRequest) {
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  const network = req.nextUrl.searchParams.get("network") ?? "";
  try {
    return NextResponse.json(await getDepositAddress(token, network));
  } catch (err) {
    return NextResponse.json({ error: "deposit address unavailable" }, { status: httpStatusFor(err) });
  }
}
