import "server-only";

import { bffRead, type ServerRead } from "@/shared/api/server/bff";
import type { Wallet } from "@/shared/contracts";

// The wallet read for server components — the same endpoint `wallet-client.ts` reads from
// the browser, so its answer can seed `walletResource` directly.

/**
 * The fields the wallet screens index into. Every proto3 JSON field is optional, so this
 * only refuses what would break them: a non-object, or a collection that is not an array.
 */
function isWallet(body: unknown): body is Wallet {
  if (typeof body !== "object" || body === null) return false;
  const { balance, deposit_addresses, withdrawable } = body as Record<string, unknown>;
  return (
    (balance === undefined || (typeof balance === "object" && balance !== null)) &&
    (deposit_addresses === undefined || Array.isArray(deposit_addresses)) &&
    (withdrawable === undefined || Array.isArray(withdrawable))
  );
}

export function readWallet(): Promise<ServerRead<Wallet> | null> {
  return bffRead("/api/wallet", isWallet);
}
