// Client-side companion to csrf.ts: read the readable `ev_csrf` cookie and build
// the double-submit header for mutating BFF requests. Standalone (no server
// imports); tries both cookie names since the `__Host-` prefix is env-dependent.

const CSRF_HEADER = "x-ev-csrf";

function readCookie(name: string): string | null {
  const prefix = `${name}=`;
  const found = document.cookie.split("; ").find((entry) => entry.startsWith(prefix));
  return found ? decodeURIComponent(found.slice(prefix.length)) : null;
}

export function csrfHeader(): Record<string, string> {
  // Prefer the `__Host-` (host-locked) cookie so a subdomain-injected plain
  // `ev_csrf` can't shadow it; fall back to the dev (unprefixed) name.
  const token = readCookie("__Host-ev_csrf") ?? readCookie("ev_csrf");
  return token ? { [CSRF_HEADER]: token } : {};
}
