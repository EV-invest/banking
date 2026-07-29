"use client";

import { Check, Copy, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";

import { Skeleton } from "@evinvest/uikit";

import { fetchDepositAddress, fetchWallet } from "@/entities/wallet/api/wallet-client";
import type { DepositAddress, Wallet } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { tonFriendlyAddress } from "@/shared/lib/ton-address";
import { TipAnchor } from "@/shared/tips";
import { isEvmRail, networkLabel } from "@/views/wallet/lib/format";
import { DepositQr } from "@/views/wallet/ui/deposit-qr";
import { NetworkSegments } from "@/views/wallet/ui/network-segments";
import { FieldLabel, WALLET_CARD, WALLET_CTA, WalletScreen } from "@/views/wallet/ui/wallet-chrome";

// Top up the balance with crypto (Figma `cabinet/wallet/deposit` + `cabinet/mobile/wallet/deposit`):
// pick a rail, then copy the address it maps to. The address is fetched per rail from the BFF —
// the rails on offer come from the wallet response, so an unwatched network never appears.
export function DepositView({ initialNetwork }: { initialNetwork?: string }) {
  const [wallet, setWallet] = useState<Wallet | null | undefined>(undefined);
  const [selected, setSelected] = useState<string | null>(initialNetwork ?? null);
  const [address, setAddress] = useState<DepositAddress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    fetchWallet()
      .then((w) => setWallet(w))
      .catch((e: Error) => {
        setWallet(null);
        setError(e.message);
      });
  }, []);

  const networks = (wallet?.deposit_addresses ?? []).map((a) => a.network ?? "").filter(Boolean);
  // A `?network=` that no longer maps to a live rail falls back to the first one on offer.
  const network = (selected && networks.includes(selected) ? selected : null) ?? networks[0] ?? "";

  useEffect(() => {
    if (!network) return;
    let active = true;
    fetchDepositAddress(network)
      .then((a) => active && setAddress(a))
      .catch((e: Error) => active && setError(e.message));
    return () => {
      active = false;
    };
  }, [network]);

  // Reset the displayed address/error in the (event-handler) network switch, not the
  // effect, so the effect performs no synchronous state update.
  const selectNetwork = (next: string) => {
    setSelected(next);
    setAddress(null);
    setError(null);
    setCopied(false);
  };

  // TON's raw `workchain:hex` form is valid but wallet-hostile — show the friendly
  // non-bounceable UQ… form (an uninitialized deposit wallet bounces EQ… sends).
  const rawAddress = address?.address ?? "";
  const displayAddress = network === "ton" ? (tonFriendlyAddress(rawAddress, { testnet: address?.is_testnet }) ?? rawAddress) : rawAddress;

  const copy = () => {
    if (!displayAddress) return;
    void navigator.clipboard.writeText(displayAddress);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  // Other EVM rails on offer: their `0x` deposit addresses look identical to this one, so a
  // BEP20/Polygon mix-up sends to a valid-but-unwatched address. Name them explicitly.
  const evmSiblings = isEvmRail(network) ? networks.filter((n) => n !== network && isEvmRail(n)).map(networkLabel) : [];
  const label = networkLabel(network);

  return (
    <WalletScreen title="Deposit USDT" subtitle="Receive funds into your wallet" back="/wallet">
      <div className="flex w-full flex-col gap-[14px] lg:max-w-[640px] lg:gap-5">
        {wallet === undefined ? (
          <>
            <Skeleton className="h-[104px] rounded-[14px]" />
            <Skeleton className="h-[360px] rounded-[14px]" />
          </>
        ) : networks.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {error ?? "No deposit rails are available right now — check back soon."}
          </p>
        ) : (
          <>
            <div className={cn(WALLET_CARD, "flex flex-col gap-[14px] p-[18px] lg:p-6")}>
              <FieldLabel>
                SELECT NETWORK
                <TipAnchor anchor="wallet.deposit.network" />
              </FieldLabel>
              <NetworkSegments networks={networks} value={network} onChange={selectNetwork} label="Deposit network" />
            </div>

            <div className={cn(WALLET_CARD, "flex flex-col items-center gap-4 p-[18px] lg:gap-[18px] lg:p-6")}>
              <p className="flex items-center gap-1.5 text-sm font-semibold text-foreground lg:text-[15px]">
                Your {label} deposit address
                <TipAnchor anchor="wallet.deposit.address" />
              </p>

              {error && !displayAddress && <p className="text-[13px] text-destructive">{error}</p>}

              {address === null ? (
                <>
                  <Skeleton className="size-40 rounded-xl lg:size-[180px] lg:rounded-[18px]" />
                  <Skeleton className="h-10 w-full rounded-lg" />
                </>
              ) : displayAddress ? (
                <>
                  <DepositQr value={displayAddress} />
                  <div className="flex w-full items-center justify-between gap-2 rounded-lg border border-border bg-input px-3 py-2.5 lg:py-2.5 lg:pl-3.5 lg:pr-2">
                    <code className="min-w-0 flex-1 break-all font-sans text-xs text-foreground lg:text-[13px]">{displayAddress}</code>
                    <button type="button" onClick={copy} className={cn(WALLET_CTA, "hidden shrink-0 gap-1.5 px-3.5 py-[7px] text-xs lg:flex")}>
                      {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
                      {copied ? "Copied" : "Copy"}
                    </button>
                  </div>
                  <button type="button" onClick={copy} className={cn(WALLET_CTA, "w-full gap-1.5 py-2.5 text-[13px] lg:hidden")}>
                    {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
                    {copied ? "Copied" : "Copy address"}
                  </button>
                  <p className="flex items-center justify-center gap-1.5 text-center text-[11px] text-muted-foreground lg:text-xs">
                    Credited to your one balance after {address.min_confirmations} network confirmations.
                    <TipAnchor anchor="wallet.deposit.min-confirmations" />
                  </p>
                </>
              ) : (
                <p className="text-center text-[13px] text-muted-foreground">
                  A {label} deposit address isn&apos;t available yet — this rail is still being provisioned. Check back soon.
                </p>
              )}
            </div>

            <div className="flex gap-2.5 rounded-[14px] border border-main-accent-t3 bg-main-accent-t3/[0.06] px-4 py-3.5 lg:gap-3 lg:px-[18px] lg:py-4">
              <TriangleAlert className="mt-px size-4 shrink-0 text-main-accent-t3" />
              <div className="flex min-w-0 flex-col gap-1">
                {/* `wallet.deposit.rail-hazard` is a section-type tip (a descriptor block, not an
                    inline ⓘ) — this card already carries that copy, so it isn't anchored here. */}
                <p className="text-[13px] font-semibold text-foreground">Network warning</p>
                <p className="text-xs text-main-mist">
                  Only send USDT on the {label} network to this address. Funds sent on any other network will be lost permanently.
                  {evmSiblings.length > 0 && (
                    <>
                      {" "}
                      This {label} address is <strong>not</strong> your {evmSiblings.join(" / ")} address — even though both start with <code>0x</code>. USDT sent on{" "}
                      {evmSiblings.join(" or ")} will not be credited.
                    </>
                  )}
                </p>
              </div>
            </div>
          </>
        )}
      </div>
    </WalletScreen>
  );
}
