import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  // gRPC clients are Node-native and must not be bundled into route handlers —
  // the BFF calls the hub's tonic backend from these packages server-side.
  serverExternalPackages: ["@grpc/grpc-js", "@grpc/proto-loader"],
};

export default nextConfig;
