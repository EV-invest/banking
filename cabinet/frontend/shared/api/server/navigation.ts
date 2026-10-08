import "server-only";

import { headers } from "next/headers";

/**
 * Whether this render answers a client-side navigation (a `<Link>`, `router.push` or
 * `router.replace`) rather than a document load.
 *
 * A screen seeds its first HTML from the server because, on a document load, nothing in
 * the browser can read before hydration. A client-side navigation is the opposite case: the
 * browser cache is already up, its reads are warmed on hover (`application/prefetch.ts`),
 * and the screen's fallback starts its own read the moment it mounts — so a server read
 * there is a second request for the same answer, and on Settings, where every rail click
 * is a `router.replace`, one per click.
 *
 * Told apart by `Sec-Fetch-Mode`, which the browser sets and page code cannot: `navigate`
 * for a document, `cors`/`same-origin` for the router's fetch of the RSC payload. Next's
 * own `RSC` header would say the same, but it is stripped before `headers()` sees it. A
 * browser that sends no `Sec-Fetch-Mode` at all is treated as a document load — the read
 * this skips is an optimisation, so the safe side is to make it.
 */
export async function isClientNavigation(): Promise<boolean> {
  const mode = (await headers()).get("sec-fetch-mode");
  return mode !== null && mode !== "navigate";
}
