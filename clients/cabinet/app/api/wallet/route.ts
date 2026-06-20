import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { getWallet, httpStatusFor } from "@/shared/api/wallet";

// The signed-in user's wallet: one unified lifecycle balance, deposit rails, and
// per-rail withdraw options, read live from the hub (TigerBeetle-authoritative). The
// user's access token never leaves the BFF — it is attached to the hub gRPC call here.
export async function GET(req: NextRequest) {
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  try {
    return NextResponse.json(await getWallet(token));
  } catch (err) {
    return NextResponse.json({ error: "wallet unavailable" }, { status: httpStatusFor(err) });
  }
}
