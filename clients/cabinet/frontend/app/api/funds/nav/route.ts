import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { getFundNav, httpStatusFor } from "@/shared/api/funds";

// The current NAV (price per share) of a fund on `?service=`, plus freshness.
export async function GET(req: NextRequest) {
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  const service = req.nextUrl.searchParams.get("service") ?? "";
  try {
    return NextResponse.json(await getFundNav(token, service));
  } catch (err) {
    return NextResponse.json({ error: "fund nav unavailable" }, { status: httpStatusFor(err) });
  }
}
