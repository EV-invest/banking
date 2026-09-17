// Which of the two stories the sign-in page tells (#391).
//
// A landing CTA sends a newcomer with `?intent=signup`; the proxy's signed-out bounce,
// the account chip and an old bookmark arrive with nothing, which means "login". The
// auth flow is the same Google OAuth either way — the intent changes the copy around
// the button, never the button — so it lives in the query string of the one page rather
// than on a `/signup` route whose only job would be to redirect here. It is read by the
// page and goes no further: the shell's `/api/auth/login` only ever sees `returnTo`.
//
// Pure and browser-safe, like `return-to.ts` beside it.

import { safeReturnTo } from "./return-to.ts";

export const LOGIN_INTENTS = ["login", "signup"] as const;
export type LoginIntent = (typeof LOGIN_INTENTS)[number];

/** The page's state for a raw `?intent=`. Anything unrecognised is a returning reader. */
export function loginIntent(raw: string | undefined): LoginIntent {
  return raw === "signup" ? "signup" : "login";
}

/**
 * The login page itself, in `intent`, for the "already have an account? / new here?"
 * switch. Zone-relative, for `shared/ui/cabinet-link` to prefix. `returnTo` rides along
 * unchanged, so flipping the copy never loses the page the reader was heading to; a bad
 * one is dropped rather than repeated.
 */
export function loginPageHref(intent: LoginIntent, returnTo: string | null | undefined): `/${string}` {
  const params = new URLSearchParams();
  if (intent !== "login") params.set("intent", intent);
  const dest = safeReturnTo(returnTo ?? null);
  if (dest !== "/") params.set("returnTo", dest);
  const query = params.toString();
  return query ? `/login?${query}` : "/login";
}
