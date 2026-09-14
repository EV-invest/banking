// How a book route is spelled on the wire: the path plus whichever query parameters the
// caller actually has. Pure and import-free so the rule that matters — an absent
// parameter is ABSENT, never `?limit=undefined` — is pinned by a test rather than by the
// BFF's 400.

export type QueryValue = string | number | undefined;

/**
 * `/api/book/trades` + `{ service: "arb", limit: 40 }` → `/api/book/trades?service=arb&limit=40`.
 *
 * Every value goes through `encodeURIComponent`: a service id is operator-typed, and the
 * one that carries a `/` or a `&` must arrive intact rather than split the query.
 */
export function bookPath(route: `/${string}`, params: Record<string, QueryValue>): `/${string}` {
  const parts: string[] = [];
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === "") continue;
    parts.push(`${encodeURIComponent(key)}=${encodeURIComponent(String(value))}`);
  }
  return parts.length === 0 ? route : `${route}?${parts.join("&")}`;
}
