/**
 * Read one field off a body of unknown shape.
 *
 * Every body this slice reads comes off the wire, so they are probed rather than asserted:
 * `as StartResponse` on a `redirect_url` that arrived as `null` would put `"null"` in
 * `window.location`. The narrowers here are the whole dependency budget of that probing —
 * a runtime schema library would be a new package in `package.json` for four shapes.
 */
export function readString(body: unknown, key: string): string | null {
  const value = readField(body, key);
  return typeof value === "string" && value.length > 0 ? value : null;
}

/**
 * Finite numbers only. `created_at` arrives as unix SECONDS; `NaN` or `Infinity` would
 * survive a `typeof` check and reach a date formatter as "Invalid Date".
 */
export function readNumber(body: unknown, key: string): number | null {
  const value = readField(body, key);
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/** Strict: a missing `resumable` is not `false`, it is an answer we did not get. */
export function readBoolean(body: unknown, key: string): boolean | null {
  const value = readField(body, key);
  return typeof value === "boolean" ? value : null;
}

/**
 * A nested object, or `null` for anything else — including JSON `null`, which is how the
 * status route says "no case is running" and is therefore not a parse failure.
 */
export function readObject(body: unknown, key: string): unknown {
  const value = readField(body, key);
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
}

/** True when the key is present at all, whatever it holds — `null` included. */
export function hasField(body: unknown, key: string): boolean {
  return typeof body === "object" && body !== null && key in body;
}

function readField(body: unknown, key: string): unknown {
  if (typeof body !== "object" || body === null || !(key in body)) return undefined;
  return (body as Record<string, unknown>)[key];
}
