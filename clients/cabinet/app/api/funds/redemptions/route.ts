import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { httpStatusFor, listRedemptions } from "@/shared/api/funds";

// The signed-in user's redemptions, newest first.
export async function GET(req: NextRequest) {
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  try {
    return NextResponse.json(await listRedemptions(token));
  } catch (err) {
    return NextResponse.json({ error: "redemptions unavailable" }, { status: httpStatusFor(err) });
  }
}
