// BFF → hub WalletService bridge (server-side only).
//
// The cabinet BFF proxies the browser's wallet requests to the hub's tonic backend.
// Like ./grpc.ts and ./auth.ts it loads the proto at runtime with @grpc/proto-loader
// (no TS codegen on the wire — the generated `@/shared/contracts` types describe the
// same messages). Unlike those, these RPCs are authenticated: each carries the user's
// hub access token as `authorization: Bearer …` metadata (the hub's inbound auth layer
// reads exactly that). Import only from route handlers (Node runtime).

import path from "node:path";

import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

import type { DepositAddress, Wallet, Withdrawal, WithdrawalList } from "@/shared/contracts";

const GRPC_ADDR = process.env.GRPC_ADDR ?? "127.0.0.1:50051";
const PROTO_DIR = process.env.GRPC_PROTO_DIR ?? path.join(process.cwd(), "..", "..", "contracts", "proto");

type Cb<T> = (err: grpc.ServiceError | null, res: T) => void;

interface WalletClient extends grpc.Client {
  GetWallet(req: Record<string, never>, meta: grpc.Metadata, cb: Cb<Wallet>): void;
  GetDepositAddress(req: { network: string }, meta: grpc.Metadata, cb: Cb<DepositAddress>): void;
  RequestWithdrawal(req: { network: string; address: string; amount: string }, meta: grpc.Metadata, cb: Cb<Withdrawal>): void;
  ListWithdrawals(req: Record<string, never>, meta: grpc.Metadata, cb: Cb<WithdrawalList>): void;
}

type WalletClientCtor = new (address: string, credentials: grpc.ChannelCredentials) => WalletClient;

let cached: WalletClient | undefined;

function walletClient(): WalletClient {
  if (cached) return cached;
  const definition = protoLoader.loadSync("banking/v1/wallet.proto", {
    includeDirs: [PROTO_DIR],
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
    oneofs: true,
  });
  const pkg = grpc.loadPackageDefinition(definition) as unknown as {
    banking: { v1: { WalletService: WalletClientCtor } };
  };
  cached = new pkg.banking.v1.WalletService(GRPC_ADDR, grpc.credentials.createInsecure());
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

/** The caller's wallet — per-network balances + deposit addresses. */
export function getWallet(token: string): Promise<Wallet> {
  return call((cb) => walletClient().GetWallet({}, bearer(token), cb));
}

/** The caller's deposit address on a network (+ min confirmations). */
export function getDepositAddress(token: string, network: string): Promise<DepositAddress> {
  return call((cb) => walletClient().GetDepositAddress({ network }, bearer(token), cb));
}

/** Open a withdrawal of free balance to an external address (two-phase saga). */
export function requestWithdrawal(token: string, body: { network: string; address: string; amount: string }): Promise<Withdrawal> {
  return call((cb) => walletClient().RequestWithdrawal(body, bearer(token), cb));
}

/** The caller's withdrawals, newest first. */
export function listWithdrawals(token: string): Promise<WithdrawalList> {
  return call((cb) => walletClient().ListWithdrawals({}, bearer(token), cb));
}

/** Map a hub gRPC status onto an HTTP status for the BFF response. */
export function httpStatusFor(err: unknown): number {
  switch ((err as grpc.ServiceError | undefined)?.code) {
    case grpc.status.UNAUTHENTICATED:
      return 401;
    case grpc.status.PERMISSION_DENIED:
      return 403;
    case grpc.status.INVALID_ARGUMENT:
      return 400;
    case grpc.status.NOT_FOUND:
      return 404;
    case grpc.status.ALREADY_EXISTS:
      return 409;
    default:
      return 502;
  }
}

/** The hub's client-safe error detail (e.g. "insufficient available balance"). */
export function errorDetail(err: unknown): string {
  return (err as grpc.ServiceError | undefined)?.details || "request failed";
}
