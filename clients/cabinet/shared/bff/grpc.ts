// BFF → hub gRPC bridge (server-side only).
//
// The cabinet BFF is the single egress to the hub's tonic backend. It loads the
// proto at runtime with @grpc/proto-loader and calls the service directly — no
// TypeScript codegen step. `tonic` + `tonic-build` generate the Rust side on
// every `cargo build`; the TS BFF reads the same `contracts/proto`. Keep this
// imported only from route handlers (Node runtime).

import path from "node:path";

import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

const GRPC_ADDR = process.env.GRPC_ADDR ?? "127.0.0.1:50051";
const PROTO_DIR = process.env.GRPC_PROTO_DIR ?? path.join(process.cwd(), "..", "..", "contracts", "proto");

interface CheckResponse {
  status: string;
}

interface HealthClient extends grpc.Client {
  Check(request: Record<string, never>, callback: (err: grpc.ServiceError | null, res: CheckResponse) => void): void;
}

type HealthClientCtor = new (address: string, credentials: grpc.ChannelCredentials) => HealthClient;

let cached: HealthClient | undefined;

function healthClient(): HealthClient {
  if (cached) return cached;
  const definition = protoLoader.loadSync("banking/v1/health.proto", {
    includeDirs: [PROTO_DIR],
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
    oneofs: true,
  });
  const pkg = grpc.loadPackageDefinition(definition) as unknown as {
    banking: { v1: { HealthService: HealthClientCtor } };
  };
  cached = new pkg.banking.v1.HealthService(GRPC_ADDR, grpc.credentials.createInsecure());
  return cached;
}

/** Call the hub's `HealthService.Check`. Rejects if the backend is unreachable. */
export function checkHealth(): Promise<CheckResponse> {
  const client = healthClient();
  return new Promise((resolve, reject) => {
    client.Check({}, (err, res) => (err ? reject(err) : resolve(res)));
  });
}
