// Keep a post-login redirect target same-origin to defeat open-redirects. Pure and
// browser-safe — used by the login view to build the `/api/auth/login?returnTo=…` link.
// The cabinet backend re-validates this server-side; this is the UX/defense-in-depth copy.

export function safeReturnTo(raw: string | null): string {
  if (!raw || !raw.startsWith("/")) return "/";
  // Reject protocol-relative ("//evil", "/\evil") and any backslash, which some browsers
  // normalize to "/" — both can escape to another origin.
  if (raw[1] === "/" || raw[1] === "\\" || raw.includes("\\")) return "/";
  return raw;
}
