import { NextResponse, type NextRequest } from "next/server";
import {
  experiments,
  cookieName,
  pickVariant,
  type ExperimentKey,
} from "@/shared/config/experiments";

// Next 16: this file MUST be named `proxy.ts` with a named `proxy` export
// (renamed from `middleware`). Runtime is nodejs (edge unsupported), which is
// fine — Math.random bucketing and cookie writes need no edge APIs.

const COOKIE_MAX_AGE = 60 * 60 * 24 * 30; // 30 days

// Assign a sticky variant cookie per experiment on first visit. New assignments
// are written to BOTH the forwarded request (so this same render's `cookies()`
// reads them — no first-paint bias) and the response (so the browser persists
// them for subsequent visits).
export function proxy(request: NextRequest) {
  const assigned: Array<{ name: string; value: string }> = [];

  for (const key of Object.keys(experiments) as ExperimentKey[]) {
    const name = cookieName(key);
    if (!request.cookies.get(name)) {
      const value = pickVariant(key);
      request.cookies.set(name, value);
      assigned.push({ name, value });
    }
  }

  const response = NextResponse.next({
    request: { headers: request.headers },
  });

  for (const { name, value } of assigned) {
    response.cookies.set(name, value, {
      maxAge: COOKIE_MAX_AGE,
      sameSite: "lax",
      httpOnly: false,
      path: "/",
    });
  }

  return response;
}

export const config = {
  matcher: ["/((?!api|_next/static|_next/image|favicon.ico).*)"],
};
