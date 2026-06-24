import type { NextConfig } from "next";

// The BFF now lives in a separate Rust service (`clients/cabinet/backend`). The browser
// keeps calling same-origin `/api/*`; Next proxies those to the backend so the
// `__Host-`/HttpOnly session cookie + CSRF model stays same-origin. In production the
// same apex domain routes `/api/*` to the backend (this rewrite is the dev/local form).
const BACKEND = process.env.CABINET_BACKEND_URL ?? "http://127.0.0.1:4000";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  async rewrites() {
    return [{ source: "/api/:path*", destination: `${BACKEND}/api/:path*` }];
  },
};

// No build-time Sentry wrapper here: @evinvest/error-monitoring's `./next` export
// is ESM-only, but Next loads next.config.ts as CJS, so `withSentry` can't be
// imported in this file. Runtime error capture is wired instead in
// `instrumentation.ts` (server) and `ErrorMonitoringProvider` (browser); only
// build-time source-map upload (which needs SENTRY_* secrets) is forgone.
export default nextConfig;
