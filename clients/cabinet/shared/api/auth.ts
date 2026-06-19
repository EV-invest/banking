// BFF → hub AuthService bridge (server-side only).
//
// The cabinet is the OAuth confidential client: it runs the browser-facing
// authorize redirect, then calls these server-to-server gRPC routes on the hub's
// auth task (AUTH_GRPC_ADDR, default :50052) to exchange the Google code for the
// hub's own tokens, rotate them, and revoke on logout. Loads auth.proto at runtime
// like ./grpc.ts — no TS codegen. This is transport only; the auth *flow* lives in
// features/auth and the session it opens in entities/session.

import path from "node:path";

import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

const AUTH_GRPC_ADDR = process.env.AUTH_GRPC_ADDR ?? "127.0.0.1:50052";
const PROTO_DIR = process.env.GRPC_PROTO_DIR ?? path.join(process.cwd(), "..", "..", "contracts", "proto");

/** A first-party token pair + principal snapshot, as the hub returns it. */
export interface TokenResponse {
  access_token: string;
  access_expires_at: string; // unix seconds (proto-loader longs: String)
  refresh_token: string;
  refresh_expires_at: string;
  user: { user_id: string; email: string; status: string; token_version: string };
}

interface AuthClient extends grpc.Client {
  Exchange(req: { auth_code: string; code_verifier: string; redirect_uri: string; nonce: string }, cb: Callback<TokenResponse>): void;
  Refresh(req: { refresh_token: string }, cb: Callback<TokenResponse>): void;
  Logout(req: { refresh_token: string; revoke_all: boolean }, cb: Callback<Record<string, never>>): void;
}

type Callback<T> = (err: grpc.ServiceError | null, res: T) => void;
type AuthClientCtor = new (address: string, credentials: grpc.ChannelCredentials) => AuthClient;

let cached: AuthClient | undefined;

function authClient(): AuthClient {
  if (cached) return cached;
  const definition = protoLoader.loadSync("banking/v1/auth.proto", {
    includeDirs: [PROTO_DIR],
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
    oneofs: true,
  });
  const pkg = grpc.loadPackageDefinition(definition) as unknown as {
    banking: { v1: { AuthService: AuthClientCtor } };
  };
  // Server-to-server on a trusted network; the hub speaks plaintext gRPC behind
  // the deployment's mTLS/service mesh.
  cached = new pkg.banking.v1.AuthService(AUTH_GRPC_ADDR, grpc.credentials.createInsecure());
  return cached;
}

function call<T>(invoke: (cb: Callback<T>) => void): Promise<T> {
  return new Promise((resolve, reject) => {
    invoke((err, res) => (err ? reject(err) : resolve(res)));
  });
}

/** Exchange a Google authorization code (with its PKCE verifier) for hub tokens. */
export function exchange(req: { auth_code: string; code_verifier: string; redirect_uri: string; nonce: string }): Promise<TokenResponse> {
  return call((cb) => authClient().Exchange(req, cb));
}

/** Rotate a refresh token for a fresh access+refresh pair. */
export function refresh(refresh_token: string): Promise<TokenResponse> {
  return call((cb) => authClient().Refresh({ refresh_token }, cb));
}

/** Revoke a refresh family (and, with revokeAll, the user's token_version). */
export function logout(refresh_token: string, revokeAll = false): Promise<void> {
  return call<Record<string, never>>((cb) => authClient().Logout({ refresh_token, revoke_all: revokeAll }, cb)).then(() => undefined);
}
