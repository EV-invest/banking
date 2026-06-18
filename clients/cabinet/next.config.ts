import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  // gRPC clients are Node-native and must not be bundled into route handlers —
  // the BFF calls the hub's tonic backend from these packages server-side.
  serverExternalPackages: ["@grpc/grpc-js", "@grpc/proto-loader"],
};

// No build-time Sentry wrapper here: @evinvest/error-monitoring's `./next` export
// is ESM-only, but Next loads next.config.ts as CJS, so `withSentry` can't be
// imported in this file. Runtime error capture is wired instead in
// `instrumentation.ts` (server) and `ErrorMonitoringProvider` (browser); only
// build-time source-map upload (which needs SENTRY_* secrets) is forgone.
export default nextConfig;
