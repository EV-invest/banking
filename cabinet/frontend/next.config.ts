import type { NextConfig } from "next";

import { BASE_PATH } from "./shared/config/base-path";
import { staticSecurityHeaders } from "./shared/config/security";

// The BFF now lives in a separate Rust service (`cabinet/backend`). The browser
// keeps calling same-origin `/api/*`; Next proxies those to the backend so the
// `__Host-`/HttpOnly session cookie + CSRF model stays same-origin. In production the
// same apex domain routes `/api/*` to the backend (this rewrite is the dev/local form).
const BACKEND = process.env.CABINET_BACKEND_URL ?? "http://127.0.0.1:4000";

const nextConfig: NextConfig = {
  reactStrictMode: true,
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
  // Request-invariant hardening on every response. The nonce-bearing CSP itself
  // is set per-request in proxy.ts; see shared/config/security.ts.
  async headers() {
    return [{ source: "/:path*", headers: staticSecurityHeaders() }];
  },
};

// No build-time Sentry wrapper here: @evinvest/error-monitoring's `./next` export
// is ESM-only, but Next loads next.config.ts as CJS, so `withSentry` can't be
// imported in this file. Runtime error capture is wired instead in
// `instrumentation.ts` (server) and `ErrorMonitoringProvider` (browser); only
// build-time source-map upload (which needs SENTRY_* secrets) is forgone.
export default nextConfig;
