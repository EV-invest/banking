import type { NextConfig } from "next";

import { BASE_PATH } from "./shared/config/base-path";

// This file deliberately reads its own env directly instead of routing
// through ./config (backed by @evinvest/settings) or shared/config/security.ts
// (which imports ./config for IN_PRODUCTION). Next's next-config-ts
// transpiler bundles this file's static local imports into a single
// next.config.compiled.js, so importing either — even lazily via a dynamic
// import(), which the transpiler doesn't follow and which has no loader for
// bare .ts paths at runtime — drags in @evinvest/settings, an ESM-only
// package (its exports map has no "require" condition) that breaks
// `next build` with ERR_PACKAGE_PATH_NOT_EXPORTED. Same class of problem as
// the Sentry note below, same resolution: forgo full integration here.
function requiredEnv(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`missing required env var ${name}`);
  return value;
}
// Where next dev/production proxies same-origin /api/* to the Rust BFF —
// baked into routes-manifest.json at build time, so it must resolve
// synchronously here (see shared/config/base-path.ts for the analogous
// basePath note).
const BACKEND = requiredEnv("CABINET_BACKEND_URL");
const IN_PRODUCTION = process.env.NODE_ENV === "production";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  // Self-contained production server (.next/standalone) — the container runs
  // `node server.js` without an `npm install` (mirrors the conductor's setup).
  output: "standalone",
  // Multi-zone mount: the cabinet lives under the conductor's domain at
  // /cabinet. basePath prefixes every page AND /_next asset with it, so nothing
  // collides with the conductor's own routes/assets; the conductor rewrites
  // /cabinet/* to this deployment. The rewrite source below is basePath-aware
  // (externally it matches /cabinet/api/*); hand-built browser URLs go through
  // shared/config/base-path.ts.
  basePath: BASE_PATH,
  async rewrites() {
    return [{ source: "/api/:path*", destination: `${BACKEND}/api/:path*` }];
  },
  // Request-invariant hardening on every response, mirroring
  // shared/config/security.ts's staticSecurityHeaders() (kept in sync by
  // hand — see the module comment above for why that helper can't be
  // imported here). The nonce-bearing CSP itself is set per-request in
  // proxy.ts, which imports shared/config/security.ts directly (a normal
  // ESM runtime context, not this file's CJS-transpiled one).
  async headers() {
    const headers = [
      { key: "X-Frame-Options", value: "DENY" },
      { key: "X-Content-Type-Options", value: "nosniff" },
      { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
    ];
    if (IN_PRODUCTION) {
      headers.push({
        key: "Strict-Transport-Security",
        value: "max-age=63072000; includeSubDomains; preload",
      });
    }
    return [{ source: "/:path*", headers }];
  },
};

// No build-time Sentry wrapper here: @evinvest/error-monitoring's `./next` export
// is ESM-only, but Next loads next.config.ts as CJS, so `withSentry` can't be
// imported in this file. Runtime error capture is wired instead in
// `instrumentation.ts` (server) and `ErrorMonitoringProvider` (browser); only
// build-time source-map upload (which needs SENTRY_* secrets) is forgone.
export default nextConfig;
