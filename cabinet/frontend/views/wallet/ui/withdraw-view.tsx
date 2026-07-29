"use client";

import { Clock, Loader2, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Skeleton } from "@evinvest/uikit";

import { fetchWallet, submitWithdrawal } from "@/entities/wallet/api/wallet-client";
import type { NetworkWithdrawable, Wallet, Withdrawal } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { formatUsdt, fromBaseUnits, networkLabel, shortAddress, subUsdt, toBaseUnits } from "@/views/wallet/lib/format";
import { NetworkSegments } from "@/views/wallet/ui/network-segments";
import { FieldLabel, WALLET_CARD, WALLET_CTA, WALLET_CTA_GHOST, WalletScreen } from "@/views/wallet/ui/wallet-chrome";

const FIELD = "w-full rounded-lg border border-border bg-input px-3 py-2.5 text-[13px] text-foreground placeholder:text-main-mist/50 focus:outline-none focus:ring-1 focus:ring-ring";

// What the user reviewed, frozen at the "Review" click — Confirm submits exactly this
// even if a wallet refetch changes the live selection underneath the open confirm.
interface ReviewedWithdrawal {
  network: string;
  address: string;
  amount: string;
  fee: string | undefined;
  instant: string | undefined;
  rails: string; // the rail list at review time — a changed list voids the review
}

function withdrawableFor(wallet: Wallet | null | undefined, network: string): NetworkWithdrawable | undefined {
  return (wallet?.withdrawable ?? []).find((w) => w.network === network);
}

// Send USDT out to an external address (Figma `cabinet/wallet/withdraw` +
// `cabinet/mobile/wallet/withdraw`). Review is a distinct step: the reviewed figures are
// snapshotted so Confirm can never submit something the user didn't see.
export function WithdrawView({ initialNetwork }: { initialNetwork?: string }) {
  const [wallet, setWallet] = useState<Wallet | null | undefined>(undefined);
  const [selected, setSelected] = useState<string | null>(initialNetwork ?? null);
  const [address, setAddress] = useState("");
  const [amount, setAmount] = useState("");
  const [confirming, setConfirming] = useState<ReviewedWithdrawal | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Withdrawal | null>(null);

  const load = useCallback(() => {
    fetchWallet()
      .then((w) => setWallet(w))
      .catch((e: Error) => {
        setWallet(null);
        setError(e.message);
      });
  }, []);

  useEffect(load, [load]);

  const networks = (wallet?.withdrawable ?? []).map((w) => w.network ?? "").filter(Boolean);
  const network = (selected && networks.includes(selected) ? selected : null) ?? networks[0] ?? "";

  const opts = withdrawableFor(wallet, network);
  // Exact base-unit math on the USDT strings — no float error on 18-dp amounts.
  const amountUnits = toBaseUnits(amount);
  const youReceive = subUsdt(amount, opts?.withdrawal_fee); // decimal string
  const queuedUnits = amountUnits - toBaseUnits(opts?.instant); // > 0 ⇒ partly queued

  // A wallet refetch that changes the rail list invalidates an open review — the
  // snapshot may point at a rail that no longer exists (guarded render-time reset).
  const rails = networks.join(",");
  if (confirming && confirming.rails !== rails) setConfirming(null);

  const submit = async () => {
    if (submitting || !confirming) return;
    setSubmitting(true);
    setError(null);
    setDone(null);
    try {
      const withdrawal = await submitWithdrawal({ network: confirming.network, address: confirming.address, amount: confirming.amount });
      setDone(withdrawal);
      setAddress("");
      setAmount("");
      load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
      setConfirming(null);
    }
  };

  const valid = network !== "" && address.trim().length > 0 && amountUnits > 0n;
  const label = networkLabel(network);

  return (
    <WalletScreen title="Withdraw USDT" subtitle="Send funds to an external address — one balance, any rail" back="/wallet">
      {wallet === undefined ? (
        <Skeleton className="h-[444px] w-full rounded-[14px] lg:max-w-[560px]" />
      ) : networks.length === 0 ? (
        <p className="text-sm text-muted-foreground">{error ?? "No withdrawal rails are available right now — check back soon."}</p>
      ) : (
        // Form 560 + review 400 side by side is the Figma at 1440; below that the content
        // column can't hold both, so the review wraps under the form rather than overflowing.
        <div className="flex flex-col gap-[14px] lg:flex-row lg:flex-wrap lg:items-start lg:gap-5">
          <div className={cn(WALLET_CARD, "flex flex-col gap-4 p-[18px] lg:w-[560px] lg:max-w-full lg:flex-none lg:p-6")}>
            <FieldLabel>
              NETWORK
              <TipAnchor anchor="wallet.withdraw.network" />
            </FieldLabel>
            <NetworkSegments
              networks={networks}
              value={network}
              onChange={(n) => {
                setSelected(n);
                setConfirming(null);
              }}
              label="Withdrawal network"
            />

            <label className="flex flex-col gap-2">
              <FieldLabel>
                DESTINATION ADDRESS
                <TipAnchor anchor="wallet.withdraw.destination" />
              </FieldLabel>
              <input
                value={address}
                onChange={(e) => {
                  setAddress(e.target.value);
                  setConfirming(null);
                }}
                placeholder={`${label} address`}
                spellCheck={false}
                className={FIELD}
              />
            </label>

            <label className="flex flex-col gap-2">
              <span className="flex items-center justify-between gap-2">
                <FieldLabel>AMOUNT</FieldLabel>
                <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                  Avail: {formatUsdt(opts?.withdrawable)}
                  <TipAnchor anchor="wallet.withdraw.available" />
                </span>
              </span>
              <span className="flex w-full items-center gap-2 rounded-lg border border-border bg-input py-2 pl-3 pr-2">
                <input
                  value={amount}
                  onChange={(e) => {
                    setAmount(e.target.value);
                    setConfirming(null);
                  }}
                  inputMode="decimal"
                  placeholder="0.00"
                  className="min-w-0 flex-1 bg-transparent text-[13px] text-foreground placeholder:text-main-mist/50 focus:outline-none"
                />
                <button
                  type="button"
                  onClick={() => {
                    setAmount(opts?.withdrawable ?? "0");
                    setConfirming(null);
                  }}
                  className="shrink-0 rounded-md border border-border px-3 py-1.5 text-xs font-medium text-main-accent-t1 transition-colors hover:bg-foreground/[0.04]"
                >
                  Max
                </button>
              </span>
            </label>

            <div className="flex flex-col gap-2.5 rounded-[10px] bg-main-surface px-3.5 py-3">
              <Row label="Network fee" value={`${formatUsdt(opts?.withdrawal_fee)} USDT`} tip="wallet.withdraw.network-fee" />
              <div className="h-px w-full bg-border" />
              <Row label="You will receive" value={`${formatUsdt(youReceive)} USDT`} tone="text-main-accent-t2" tip="wallet.withdraw.you-receive" />
            </div>

            {/* `wallet.withdraw.queueing` / `.review` are section-type tips (descriptor blocks,
                not inline ⓘ), so that copy is stated inline here rather than anchored. */}
            {queuedUnits > 0n && amountUnits > 0n && (
              <p className="text-[11px] text-main-accent-t3">
                ~{formatUsdt(fromBaseUnits(queuedUnits))} USDT exceeds instant {label} liquidity and will be queued until the rail is topped up.
              </p>
            )}
            <p className="text-[11px] text-muted-foreground">
              Min {formatUsdt(opts?.min_withdrawal)} USDT · instant {label} {formatUsdt(opts?.instant)} · rest queued
            </p>

            <button
              type="button"
              disabled={!valid || submitting}
              onClick={() => setConfirming({ network, address, amount, fee: opts?.withdrawal_fee, instant: opts?.instant, rails })}
              className={cn(WALLET_CTA, "w-full py-3 text-sm font-semibold")}
            >
              Review withdrawal
            </button>
          </div>

          <div className="flex flex-col gap-[14px] empty:hidden lg:w-[400px] lg:max-w-full lg:flex-none lg:gap-5">
            {done && (
              <div className={cn(WALLET_CARD, "flex gap-3 p-[18px] lg:p-5")}>
                <Clock className="mt-0.5 size-4 shrink-0 text-main-accent-t3" />
                <div className="min-w-0">
                  <p className="text-[13px] font-semibold text-foreground">{done.state === "queued" ? "Withdrawal queued" : "Withdrawal submitted"}</p>
                  <p className="text-xs text-main-mist">
                    {done.state === "queued"
                      ? `${formatUsdt(done.net_amount)} USDT to ${shortAddress(done.address)} is queued — it ships once the ${networkLabel(done.network)} rail is topped up.`
                      : `${formatUsdt(done.net_amount)} USDT is on its way to ${shortAddress(done.address)} — pending on-chain confirmation.`}
                  </p>
                </div>
              </div>
            )}

            {error && (
              <div className={cn(WALLET_CARD, "flex gap-3 border-destructive/50 p-[18px] lg:p-5")}>
                <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
                <div className="min-w-0">
                  <p className="text-[13px] font-semibold text-foreground">Withdrawal failed</p>
                  <p className="text-xs text-main-mist">{error}</p>
                </div>
              </div>
            )}

            {confirming && (
              <div className={cn(WALLET_CARD, "flex flex-col gap-4 p-[18px] lg:p-5")}>
                <p className="text-[15px] font-semibold text-foreground">Review withdrawal</p>
                <div className="flex flex-col gap-2.5">
                  <Row label="Network" value={networkLabel(confirming.network)} />
                  <Row label="Destination" value={shortAddress(confirming.address)} />
                  <Row label="Amount" value={`${formatUsdt(confirming.amount)} USDT`} />
                  <Row label="Network fee" value={`${formatUsdt(confirming.fee)} USDT`} />
                  <div className="h-px w-full bg-border" />
                  <Row label="You will receive" value={`${formatUsdt(subUsdt(confirming.amount, confirming.fee))} USDT`} tone="text-main-accent-t2" />
                </div>
                <p className="break-all font-mono-tech text-[11px] text-muted-foreground">To {confirming.address}</p>
                {toBaseUnits(confirming.amount) - toBaseUnits(confirming.instant) > 0n && (
                  <p className="text-[11px] text-main-accent-t3">
                    ~{formatUsdt(fromBaseUnits(toBaseUnits(confirming.amount) - toBaseUnits(confirming.instant)))} USDT exceeds instant {networkLabel(confirming.network)}{" "}
                    liquidity and will be queued until the rail is topped up.
                  </p>
                )}
                <div className="flex gap-2">
                  <button type="button" disabled={submitting} onClick={submit} className={cn(WALLET_CTA, "flex-1 gap-2 py-3 text-sm font-semibold")}>
                    {submitting && <Loader2 className="size-4 animate-spin" />}
                    Confirm withdrawal
                  </button>
                  <button type="button" disabled={submitting} onClick={() => setConfirming(null)} className={cn(WALLET_CTA_GHOST, "px-4 py-3 text-sm")}>
                    Back
                  </button>
                </div>
                <p className="text-[11px] text-muted-foreground">
                  Accepted instantly; if the {networkLabel(confirming.network)}{" "}
                  rail is short it queues until funded — you&apos;ll be notified.
                </p>
              </div>
            )}
          </div>
        </div>
      )}
    </WalletScreen>
  );
}

function Row({ label, value, tone, tip }: { label: string; value: string; tone?: string; tip?: TipKey }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
        {label}
        {tip && <TipAnchor anchor={tip} />}
      </span>
      <span className={cn("text-[13px] font-medium tabular-nums", tone ?? "text-foreground")}>{value}</span>
    </div>
  );
}
