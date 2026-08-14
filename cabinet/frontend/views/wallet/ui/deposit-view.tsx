"use client";

import { Check, Copy, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { Skeleton } from "@evinvest/uikit";

import { depositAddressResource, walletResource } from "@/entities/wallet/model/wallet-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Settled } from "@/shared/ui/motion";
import { displayAddress } from "@/shared/lib/ton-address";
import { TipAnchor } from "@/shared/tips";
import { isEvmRail, networkLabel } from "@/views/wallet/lib/format";
import { DepositQr } from "@/views/wallet/ui/deposit-qr";
import { NetworkSegments } from "@/views/wallet/ui/network-segments";
import { FieldLabel, WALLET_CARD, WALLET_CTA, WalletScreen } from "@/views/wallet/ui/wallet-chrome";

// Top up the balance with crypto (Figma `cabinet/wallet/deposit` + `cabinet/mobile/wallet/deposit`):
// pick a rail, then copy the address it maps to. The address is fetched per rail from the BFF —
// the rails on offer come from the wallet response, so an unwatched network never appears.
export function DepositView({ initialNetwork }: { initialNetwork?: string }) {
  const [selected, setSelected] = useState<string | null>(initialNetwork ?? null);
  const [copied, setCopied] = useState(false);

  const { data: wallet, error: walletError, isLoading: walletLoading } = useResource(walletResource);

  const networks = (wallet?.deposit_addresses ?? []).map((a) => a.network ?? "").filter(Boolean);
  // A `?network=` that no longer maps to a live rail falls back to the first one on offer.
  const network = (selected && networks.includes(selected) ? selected : null) ?? networks[0] ?? "";

  // Keyed on the rail, so switching back to a network already looked at re-shows its
  // address and QR immediately instead of re-fetching an address that cannot have changed.
  const { data: address, error: addressError, isLoading: addressLoading } = useResource(depositAddressResource, network);

  const error = (wallet ? null : walletError?.message) ?? (address ? null : addressError?.message) ?? null;

  // The address swap is now a consequence of the key changing; only the copy affordance
  // carries state that has to be reset by hand.
  const selectNetwork = (next: string) => {
    setSelected(next);
    setCopied(false);
  };

  // TON's raw `workchain:hex` form is valid but wallet-hostile — show the friendly
  // non-bounceable UQ… form (an uninitialized deposit wallet bounces EQ… sends).
  const shown = displayAddress(network, address?.address ?? "", { testnet: address?.is_testnet });

  const copy = () => {
    if (!shown) return;
    void navigator.clipboard.writeText(shown);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  // Other EVM rails on offer: their `0x` deposit addresses look identical to this one, so a
  // BEP20/Polygon mix-up sends to a valid-but-unwatched address. Name them explicitly.
  const evmSiblings = isEvmRail(network) ? networks.filter((n) => n !== network && isEvmRail(n)).map(networkLabel) : [];
  const label = networkLabel(network);

  return (
    <WalletScreen title="Deposit USDT" subtitle="Receive funds into your wallet" back="/wallet">
      <div className="flex w-full flex-col gap-3.5 lg:max-w-160 lg:gap-5">
        {/* The wrapper repeats the column so the two children keep their gap:
            `display: contents` would preserve the parent's layout but makes
            opacity and transform no-ops, which is the whole point here. */}
        <Settled
          className="flex flex-col gap-3.5 lg:gap-5"
          loading={walletLoading}
          skeleton={
            <>
              <Skeleton className="h-26 rounded-xl" />
              <Skeleton className="h-90 rounded-xl" />
            </>
          }
        >
          {walletLoading ? null : networks.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              {error ?? "No deposit rails are available right now — check back soon."}
            </p>
          ) : (
            <>
              <div className={cn(WALLET_CARD, "flex flex-col gap-3.5 p-4.5 lg:p-6")}>
                <FieldLabel>
                  SELECT NETWORK
                  <TipAnchor anchor="wallet.deposit.network" />
                </FieldLabel>
                <NetworkSegments networks={networks} value={network} onChange={selectNetwork} label="Deposit network" />
              </div>

              <div className={cn(WALLET_CARD, "flex flex-col items-center gap-4 p-4.5 lg:gap-4.5 lg:p-6")}>
                <p className="flex items-center gap-1.5 text-sm font-semibold text-foreground">
                  Your {label} deposit address
                  <TipAnchor anchor="wallet.deposit.address" />
                </p>

                {error && !shown && <p className="text-sm text-destructive">{error}</p>}

                {addressLoading ? (
                  <>
                    <Skeleton className="size-40 rounded-xl lg:size-45 lg:rounded-2xl" />
                    <Skeleton className="h-10 w-full rounded-lg" />
                  </>
                ) : address && shown ? (
                  <>
                    <DepositQr value={shown} />
                    <div className="flex w-full items-center justify-between gap-2 rounded-lg border border-border bg-input px-3 py-2.5 lg:py-2.5 lg:pl-3.5 lg:pr-2">
                      <code className="min-w-0 flex-1 break-all font-sans text-xs text-foreground lg:text-sm">{shown}</code>
                      <button type="button" onClick={copy} className={cn(WALLET_CTA, "hidden shrink-0 gap-1.5 px-3.5 py-2 text-xs lg:flex")}>
                        {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
                        {copied ? "Copied" : "Copy"}
                      </button>
                    </div>
                    <button type="button" onClick={copy} className={cn(WALLET_CTA, "w-full gap-1.5 py-2.5 text-sm lg:hidden")}>
                      {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
                      {copied ? "Copied" : "Copy address"}
                    </button>
                    <p className="flex items-center justify-center gap-1.5 text-center text-xs text-muted-foreground">
                      Credited to your one balance after {address.min_confirmations} network confirmations.
                      <TipAnchor anchor="wallet.deposit.min-confirmations" />
                    </p>
                  </>
                ) : (
                  <p className="text-center text-sm text-muted-foreground">
                    A {label} deposit address isn&apos;t available yet — this rail is still being provisioned. Check back soon.
                  </p>
                )}
              </div>

              <div className="flex gap-2.5 rounded-xl border border-main-accent-t3 bg-main-accent-t3/5 px-4 py-3.5 lg:gap-3 lg:px-4.5 lg:py-4">
                <TriangleAlert className="mt-px size-4 shrink-0 text-main-accent-t3" />
                <div className="flex min-w-0 flex-col gap-1">
                  {/* `wallet.deposit.rail-hazard` is a section-type tip (a descriptor block, not an
                      inline ⓘ) — this card already carries that copy, so it isn't anchored here. */}
                  <p className="text-sm font-semibold text-foreground">Network warning</p>
                  <p className="text-xs text-muted-foreground">
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
        </Settled>
      </div>
    </WalletScreen>
  );
}
