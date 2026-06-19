// CSRF defense for state-changing BFF routes (logout): the double-submit pattern.
// The session sets a non-HttpOnly `ev_csrf` cookie; a mutating request must echo
// that value in the `x-ev-csrf` header. A cross-site form/script can't read the
// cookie to forge the header, so a mismatch (or absence) is rejected.

import type { NextRequest } from "next/server";

import { COOKIES } from "@/shared/config/cookies";

export const CSRF_HEADER = "x-ev-csrf";

export function verifyCsrf(req: NextRequest): boolean {
  const cookie = req.cookies.get(COOKIES.csrf)?.value;
  const header = req.headers.get(CSRF_HEADER);
  return Boolean(cookie) && cookie === header;
}
