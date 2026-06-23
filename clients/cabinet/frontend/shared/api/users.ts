// BFF → hub UsersService bridge (server-side only).
//
// Mirrors ./wallet.ts: loads the proto at runtime with @grpc/proto-loader and carries
// the user's hub access token as `authorization: Bearer …` metadata. The hub resolves
// the caller from the token's `sub`, so these act on the caller's own record. Import
// only from route handlers (Node runtime).

import path from "node:path";

import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

import type { UpdateProfileRequest, UserProfile } from "@/shared/contracts";

const GRPC_ADDR = process.env.GRPC_ADDR ?? "127.0.0.1:50051";
const PROTO_DIR = process.env.GRPC_PROTO_DIR ?? path.join(process.cwd(), "..", "..", "contracts", "proto");

type Cb<T> = (err: grpc.ServiceError | null, res: T) => void;

interface UsersClient extends grpc.Client {
  GetMe(req: Record<string, never>, meta: grpc.Metadata, cb: Cb<UserProfile>): void;
  UpdateProfile(req: UpdateProfileRequest, meta: grpc.Metadata, cb: Cb<UserProfile>): void;
}

type UsersClientCtor = new (address: string, credentials: grpc.ChannelCredentials) => UsersClient;

let cached: UsersClient | undefined;

function usersClient(): UsersClient {
  if (cached) return cached;
  const definition = protoLoader.loadSync("banking/v1/users.proto", {
    includeDirs: [PROTO_DIR],
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
    oneofs: true,
  });
  const pkg = grpc.loadPackageDefinition(definition) as unknown as {
    banking: { v1: { UsersService: UsersClientCtor } };
  };
  cached = new pkg.banking.v1.UsersService(GRPC_ADDR, grpc.credentials.createInsecure());
  return cached;
}

function bearer(token: string): grpc.Metadata {
  const meta = new grpc.Metadata();
  meta.set("authorization", `Bearer ${token}`);
  return meta;
}

function call<T>(invoke: (cb: Cb<T>) => void): Promise<T> {
  return new Promise((resolve, reject) => {
    invoke((err, res) => (err ? reject(err) : resolve(res)));
  });
}

/** The caller's profile (identity + editable fields), resolved from the access token. */
export function getMe(token: string): Promise<UserProfile> {
  return call((cb) => usersClient().GetMe({}, bearer(token), cb));
}

/** Full-replace the caller's editable profile fields; returns the updated profile. */
export function updateProfile(token: string, fields: UpdateProfileRequest): Promise<UserProfile> {
  return call((cb) => usersClient().UpdateProfile(fields, bearer(token), cb));
}
