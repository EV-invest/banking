import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { httpStatusFor, listPositions } from "@/shared/api/funds";

// The signed-in user's fund positions (units, NAV, value, cost basis, P&L), read live
// from the hub. The user's access token never leaves the BFF — it is attached to the
// hub gRPC call here.
export async function GET(req: NextRequest) {
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  try {
    return NextResponse.json(await listPositions(token));
  } catch (err) {
    return NextResponse.json({ error: "positions unavailable" }, { status: httpStatusFor(err) });
  }
}
