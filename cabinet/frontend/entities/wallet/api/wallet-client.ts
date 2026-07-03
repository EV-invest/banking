// Browser → BFF wallet client. Thin typed fetchers over the BFF route handlers; the
// shapes are the proto-derived types from `@/shared/contracts`. Mutations carry the
// CSRF double-submit header. No tokens are ever seen here — the BFF holds them.

import { apiPath } from "@/shared/config/base-path";
import { csrfHeader } from "@/shared/lib/csrf-client";
import type { DepositAddress, DepositList, Wallet, Withdrawal, WithdrawalList } from "@/shared/contracts";

async function getJson<T>(url: `/${string}`): Promise<T> {
  const res = await fetch(apiPath(url), { headers: { accept: "application/json" } });
  const data = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `request failed (${res.status})`);
  return data;
}

export function fetchWallet(): Promise<Wallet> {
  return getJson<Wallet>("/api/wallet");
}

export function fetchDepositAddress(network: string): Promise<DepositAddress> {
  return getJson<DepositAddress>(`/api/wallet/deposit-address?network=${encodeURIComponent(network)}`);
}

export function fetchWithdrawals(): Promise<WithdrawalList> {
  return getJson<WithdrawalList>("/api/wallet/withdrawals");
}

export function fetchDeposits(): Promise<DepositList> {
  return getJson<DepositList>("/api/wallet/deposits");
}

export async function submitWithdrawal(body: { network: string; address: string; amount: string }): Promise<Withdrawal> {
  const res = await fetch(apiPath("/api/wallet/withdrawals"), {
    method: "POST",
    headers: { "content-type": "application/json", ...csrfHeader() },
    body: JSON.stringify(body),
  });
  const data = (await res.json().catch(() => ({}))) as Withdrawal & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `withdrawal failed (${res.status})`);
  return data;
}

export async function cancelWithdrawal(withdrawalId: string): Promise<Withdrawal> {
  const res = await fetch(apiPath("/api/wallet/withdrawals/cancel"), {
    method: "POST",
    headers: { "content-type": "application/json", ...csrfHeader() },
    body: JSON.stringify({ withdrawal_id: withdrawalId }),
  });
  const data = (await res.json().catch(() => ({}))) as Withdrawal & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `cancel failed (${res.status})`);
  return data;
}
