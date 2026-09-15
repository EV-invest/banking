/**
 * The vendor's URL is handed to us by our own identity plane, but it lands in
 * `window.location`, so it is checked here before anyone is sent to it.
 *
 * Two checks, in order of what they buy:
 *
 *  · **scheme** — a `javascript:` or `data:` URL in `window.location.assign` executes in THIS
 *    origin. One comparison keeps a confused provider from becoming an XSS in the cabinet.
 *  · **host** — the scheme check alone stops nothing a real attacker would do, because any
 *    `https:` URL passes it (#193). A swapped vendor domain or a misconfigured plane sends
 *    the user off the cabinet on a click they were told to trust.
 *
 * The host allowlist is deliberately OPTIONAL. The authoritative check is on the plane, which
 * refuses a redirect whose host is not the vendor's and answers 503 instead; this one is
 * defence in depth, and an unset `NEXT_PUBLIC_KYC_PROVIDER_HOST` must not black-hole a
 * working flow — an empty allowlist degrades to the scheme check that shipped before it.
 */
export function providerUrl(raw: string | null, allowedHosts: string | undefined): string | null {
  if (!raw) return null;
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return null;
  }
  if (url.protocol !== "https:") return null;
  const allowed = parseHosts(allowedHosts);
  if (allowed.length > 0 && !allowed.includes(url.host.toLowerCase())) return null;
  return raw;
}

/**
 * Comma-separated so a staging host can sit beside production in one variable. Hosts, not
 * origins: the scheme is already fixed to https above, and a port is part of the host.
 */
function parseHosts(value: string | undefined): readonly string[] {
  if (!value) return [];
  return value
    .split(",")
    .map((host) => host.trim().toLowerCase())
    .filter((host) => host.length > 0);
}
