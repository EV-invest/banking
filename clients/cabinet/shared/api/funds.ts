// BFF → hub FundsService bridge (server-side only).
//
// The cabinet BFF proxies the browser's fund-shares (NAV-units) requests to the hub's
// tonic backend. Like ./wallet.ts it loads the proto at runtime with @grpc/proto-loader
// (no TS codegen on the wire — the generated `@/shared/contracts` types describe the
// same messages), and each RPC carries the user's hub access token as
// `authorization: Bearer …` metadata (the hub's inbound auth layer reads exactly that).
// Self-service RPCs act on the caller's access-token `sub`. Import only from route
// handlers (Node runtime). The status/error mappers are reused from ./wallet.

import path from "node:path";

import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

import type { FundNav, Position, PositionList, Redemption, RedemptionList, Subscription } from "@/shared/contracts";

export { errorDetail, httpStatusFor } from "@/shared/api/wallet";

const GRPC_ADDR = process.env.GRPC_ADDR ?? "127.0.0.1:50051";
const PROTO_DIR = process.env.GRPC_PROTO_DIR ?? path.join(process.cwd(), "..", "..", "contracts", "proto");

type Cb<T> = (err: grpc.ServiceError | null, res: T) => void;

interface FundsClient extends grpc.Client {
  Subscribe(req: { service: string; amount: string }, meta: grpc.Metadata, cb: Cb<Subscription>): void;
  Redeem(req: { service: string; units: string }, meta: grpc.Metadata, cb: Cb<Redemption>): void;
  CancelRedemption(req: { redemption_id: string }, meta: grpc.Metadata, cb: Cb<Redemption>): void;
  GetPosition(req: { service: string }, meta: grpc.Metadata, cb: Cb<Position>): void;
  ListPositions(req: Record<string, never>, meta: grpc.Metadata, cb: Cb<PositionList>): void;
  ListRedemptions(req: Record<string, never>, meta: grpc.Metadata, cb: Cb<RedemptionList>): void;
  GetFundNav(req: { service: string }, meta: grpc.Metadata, cb: Cb<FundNav>): void;
}

type FundsClientCtor = new (address: string, credentials: grpc.ChannelCredentials) => FundsClient;

let cached: FundsClient | undefined;

function fundsClient(): FundsClient {
  if (cached) return cached;
  const definition = protoLoader.loadSync("banking/v1/funds.proto", {
    includeDirs: [PROTO_DIR],
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
    oneofs: true,
  });
  const pkg = grpc.loadPackageDefinition(definition) as unknown as {
    banking: { v1: { FundsService: FundsClientCtor } };
  };
  cached = new pkg.banking.v1.FundsService(GRPC_ADDR, grpc.credentials.createInsecure());
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

/** Subscribe `amount` of free balance into a fund — mints units at the current NAV. */
export function subscribe(token: string, body: { service: string; amount: string }): Promise<Subscription> {
  return call((cb) => fundsClient().Subscribe(body, bearer(token), cb));
}

/** Redeem `units` of a fund back to cash (accept-and-queue, settle-time priced). */
export function redeem(token: string, body: { service: string; units: string }): Promise<Redemption> {
  return call((cb) => fundsClient().Redeem(body, bearer(token), cb));
}

/** Cancel one of the caller's still-queued redemptions (returns the reserved units). */
export function cancelRedemption(token: string, redemptionId: string): Promise<Redemption> {
  return call((cb) => fundsClient().CancelRedemption({ redemption_id: redemptionId }, bearer(token), cb));
}

/** The caller's position in one fund (units, NAV, value, cost basis, P&L). */
export function getPosition(token: string, service: string): Promise<Position> {
  return call((cb) => fundsClient().GetPosition({ service }, bearer(token), cb));
}

/** All of the caller's fund positions. */
export function listPositions(token: string): Promise<PositionList> {
  return call((cb) => fundsClient().ListPositions({}, bearer(token), cb));
}

/** The caller's redemptions, newest first. */
export function listRedemptions(token: string): Promise<RedemptionList> {
  return call((cb) => fundsClient().ListRedemptions({}, bearer(token), cb));
}

/** The current NAV (price per share) for a fund, plus freshness. */
export function getFundNav(token: string, service: string): Promise<FundNav> {
  return call((cb) => fundsClient().GetFundNav({ service }, bearer(token), cb));
}
