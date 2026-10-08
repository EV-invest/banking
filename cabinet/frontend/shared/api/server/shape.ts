// The one check a server read can afford on a BFF body: it is a JSON object, and the
// collections the screens `.map` over are arrays. proto3 JSON drops an empty repeated field
// and every scalar is optional, so anything stricter would refuse healthy answers — and a
// refused body only costs the browser read the screen always made.

export function isJsonObject(body: unknown): body is Record<string, unknown> {
  return typeof body === "object" && body !== null && !Array.isArray(body);
}

/** An object whose named fields, where present, are arrays. */
export function hasOptionalLists(body: unknown, fields: readonly string[]): body is Record<string, unknown> {
  return isJsonObject(body) && fields.every((field) => body[field] === undefined || Array.isArray(body[field]));
}
