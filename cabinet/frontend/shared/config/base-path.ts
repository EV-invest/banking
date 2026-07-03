// The zone mount prefix. The cabinet is a Next.js multi-zone served under the
// conductor's domain at /cabinet (`basePath` in next.config.ts): next/link and
// rewrite sources are basePath-aware, but hand-built URLs — raw fetch() calls
// and plain <a href> — are not. Every such URL must go through apiPath()/
// withBasePath() so the request stays inside the zone instead of hitting the
// conductor's origin root.
export const BASE_PATH = "/cabinet";

export const withBasePath = (path: `/${string}`): string => `${BASE_PATH}${path}`;

/** Alias for call sites that read better as "API path" (all BFF routes). */
export const apiPath = withBasePath;
