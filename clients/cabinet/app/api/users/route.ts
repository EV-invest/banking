import { type NextRequest, NextResponse } from "next/server";

import { accessTokenFor, readSessionId } from "@/entities/session/model/session";
import { verifyCsrf } from "@/features/auth/lib/csrf";
import { getMe, updateProfile } from "@/shared/api/users";
import { errorDetail, httpStatusFor } from "@/shared/api/wallet";
import type { UpdateProfileRequest } from "@/shared/contracts";

// The caller's profile: GET reads identity + editable fields; PATCH full-replaces the
// editable fields. The user's hub access token never leaves the BFF — it is attached to
// the gRPC call here, and the hub resolves the caller from the token's `sub`.
export async function GET(req: NextRequest) {
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  try {
    return NextResponse.json(await getMe(token));
  } catch (err) {
    return NextResponse.json({ error: errorDetail(err) }, { status: httpStatusFor(err) });
  }
}

const EDITABLE = [
  "legal_name",
  "preferred_name",
  "phone",
  "date_of_birth",
  "nationality",
  "tax_residence",
  "residential_address",
  "language",
  "base_currency",
  "timezone",
] as const;

export async function PATCH(req: NextRequest) {
  if (!verifyCsrf(req)) return NextResponse.json({ error: "csrf" }, { status: 403 });
  const token = await accessTokenFor(readSessionId(req));
  if (!token) return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  const body = (await req.json().catch(() => ({}))) as Record<string, unknown>;
  // Whitelist the editable fields; coerce to string (full-replace, empty clears).
  const fields = Object.fromEntries(EDITABLE.map((k) => [k, typeof body[k] === "string" ? body[k] : ""])) as unknown as UpdateProfileRequest;
  try {
    return NextResponse.json(await updateProfile(token, fields));
  } catch (err) {
    return NextResponse.json({ error: errorDetail(err) }, { status: httpStatusFor(err) });
  }
}
