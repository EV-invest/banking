/**
 * Read one non-empty string off a body of unknown shape.
 *
 * Both bodies this slice reads — the started case and the 503 — come off the wire, so
 * they are probed rather than asserted: `as StartResponse` on a `redirect_url` that
 * arrived as `null` would put `"null"` in `window.location`.
 */
export function readString(body: unknown, key: string): string | null {
  if (typeof body !== "object" || body === null || !(key in body)) return null;
  const value = (body as Record<string, unknown>)[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}
