"use client";

import { TriangleAlert } from "lucide-react";
import Link from "next/link";
import { useEffect, useState } from "react";

import { Skeleton } from "@evinvest/uikit";

import { fetchWallet } from "@/entities/wallet/api/wallet-client";
import type { Wallet } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { formatUsdt, railMeta } from "@/views/wallet/lib/format";
import { FieldLabel, WALLET_CARD, WALLET_CTA, WALLET_CTA_GHOST, WalletScreen } from "@/views/wallet/ui/wallet-chrome";

// The wallet landing surface (Figma `cabinet/wallet` + `cabinet/mobile/wallet`): one balance
// up top, then the rails as cards — each rail is only a way in and out of that single balance,
// never a balance of its own. Deposit/withdraw are their own routes, so a rail card links
// straight into the right screen with the rail preselected.
export function WalletOverviewView() {
  const [wallet, setWallet] = useState<Wallet | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchWallet()
      .then((w) => {
        setWallet(w);
        setError(null);
      })
      .catch((e: Error) => {
        setWallet(null);
        setError(e.message);
      });
  }, []);

  const balance = wallet?.balance;
  const loading = wallet === undefined;

  const depositable = new Set((wallet?.deposit_addresses ?? []).map((a) => a.network ?? "").filter(Boolean));
  const withdrawable = new Set((wallet?.withdrawable ?? []).map((w) => w.network ?? "").filter(Boolean));
  // Union of both directions — a rail watched for deposits but not yet funded for payouts
  // still belongs on the list, with its unavailable action shown as disabled.
  const rails = [...new Set([...depositable, ...withdrawable])];

  return (
    <WalletScreen
      title="Wallet"
      subtitle="One USDT balance · networks are deposit / withdrawal rails"
      actions={
        <>
          <Link href="/wallet/deposit" className={cn(WALLET_CTA, "px-4 py-[9px] text-[13px]")}>
            Deposit
          </Link>
          <Link href="/wallet/withdraw" className={cn(WALLET_CTA_GHOST, "px-4 py-[9px] text-[13px]")}>
            Withdraw
          </Link>
          <Link href="/invest" className={cn(WALLET_CTA_GHOST, "px-4 py-[9px] text-[13px]")}>
            Allocate
          </Link>
          <Link href="/wallet/activity" className={cn(WALLET_CTA_GHOST, "px-4 py-[9px] text-[13px]")}>
            Activity
          </Link>
        </>
      }
    >
      {error && (
        <div className={cn(WALLET_CARD, "flex gap-3 border-destructive/50 p-[18px] lg:p-6")}>
          <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
          <div className="min-w-0">
            <p className="text-[13px] font-semibold text-foreground">Couldn&apos;t load your wallet</p>
            <p className="text-xs text-main-mist">{error}</p>
          </div>
        </div>
      )}

      <div className={cn(WALLET_CARD, "flex flex-col gap-4 p-5 lg:flex-row lg:items-center lg:justify-between lg:gap-6 lg:px-7 lg:py-6")}>
        <div className="flex flex-col gap-2">
          <FieldLabel className="tracking-[0.5px]">TOTAL BALANCE</FieldLabel>
          <div className="flex items-baseline gap-2">
            {loading ? (
              <Skeleton className="h-9 w-44 lg:h-[46px] lg:w-64" />
            ) : (
              <p className="text-[34px] font-semibold leading-none text-white tabular-nums lg:text-[46px]">{formatUsdt(balance?.total)}</p>
            )}
            <p className="text-[13px] font-medium text-muted-foreground lg:text-base">USDT</p>
          </div>
          {/* `wallet.balance.model` is a section-type tip (a descriptor block, not an inline ⓘ),
              so it would break this row — the chips carry the inline tips instead. */}
          <p className="hidden whitespace-nowrap text-[13px] text-main-mist lg:block">≈ ${formatUsdt(balance?.total)} · one fungible balance</p>
        </div>
        <div className="grid grid-cols-3 gap-2.5 lg:flex lg:shrink-0">
          <Chip label="AVAIL" wideLabel="AVAILABLE" dot="bg-main-accent-t2" value={balance?.available} loading={loading} tip="wallet.balance.available" />
          <Chip label="INVEST" wideLabel="INVESTED" dot="bg-main-accent-t3" value={balance?.invested} loading={loading} tip="wallet.balance.invested" />
          <Chip label="PEND WD" wideLabel="PENDING WD" dot="bg-main-accent-t1" value={balance?.pending_withdrawal} loading={loading} tip="wallet.balance.pending-withdrawal" />
        </div>
      </div>

      <div className="grid grid-cols-3 gap-2 lg:hidden">
        <Link href="/wallet/deposit" className={cn(WALLET_CTA, "py-2.5 text-[13px]")}>
          Deposit
        </Link>
        <Link href="/wallet/withdraw" className={cn(WALLET_CTA_GHOST, "py-2.5 text-[13px]")}>
          Withdraw
        </Link>
        <Link href="/invest" className={cn(WALLET_CTA_GHOST, "py-2.5 text-[13px]")}>
          Allocate
        </Link>
      </div>

      <div className="flex items-center justify-between">
        <p className="text-[15px] font-semibold text-foreground">Networks</p>
        {/* The Figma frames leave the activity screen with no entry point; this is it. */}
        <Link href="/wallet/activity" className="text-xs text-main-accent-t1 hover:underline lg:hidden">
          Activity
        </Link>
        <p className="hidden text-xs text-muted-foreground lg:block">deposit &amp; withdrawal rails</p>
      </div>

      {loading ? (
        <div className="grid gap-[14px] lg:grid-cols-2 lg:gap-5 xl:grid-cols-3">
          <Skeleton className="h-[107px] rounded-[14px]" />
          <Skeleton className="hidden h-[123px] rounded-[14px] lg:block" />
          <Skeleton className="hidden h-[123px] rounded-[14px] xl:block" />
        </div>
      ) : rails.length === 0 ? (
        <p className="text-sm text-muted-foreground">No deposit or withdrawal rails are available right now — check back soon.</p>
      ) : (
        <div className="grid gap-[14px] lg:grid-cols-2 lg:gap-5 xl:grid-cols-3">
          {rails.map((network) => (
            <RailCard key={network} network={network} canDeposit={depositable.has(network)} canWithdraw={withdrawable.has(network)} />
          ))}
        </div>
      )}
    </WalletScreen>
  );
}

// The label shortens on mobile (`AVAIL`) and spells out from `lg` up — the Figma frames use
// both, and the chip is too narrow at 390px for the long form.
function Chip({ label, wideLabel, dot, value, loading, tip }: { label: string; wideLabel: string; dot: string; value: string | undefined; loading: boolean; tip: TipKey }) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5 rounded-[10px] border border-border px-2.5 py-2.5 lg:gap-1.5 lg:px-4 lg:py-3.5">
      <span className="flex items-center gap-1.5">
        <span className={cn("size-1.5 shrink-0 rounded-full lg:size-[7px]", dot)} />
        <span className="truncate text-[10px] font-medium text-muted-foreground lg:text-[11px]">
          <span className="lg:hidden">{label}</span>
          <span className="hidden lg:inline">{wideLabel}</span>
        </span>
        <TipAnchor anchor={tip} />
      </span>
      {loading ? <Skeleton className="h-[17px] w-16 lg:h-[22px] lg:w-20" /> : <p className="truncate text-[15px] font-semibold text-foreground tabular-nums lg:text-[18px]">{formatUsdt(value)}</p>}
    </div>
  );
}

function RailCard({ network, canDeposit, canWithdraw }: { network: string; canDeposit: boolean; canWithdraw: boolean }) {
  const rail = railMeta(network);
  return (
    <div className={cn(WALLET_CARD, "flex flex-col gap-[14px] p-[18px] lg:gap-4 lg:p-5")}>
      <div className="flex items-center gap-2.5">
        <span className={cn("flex size-8 shrink-0 items-center justify-center rounded-[9px] text-[13px] font-semibold lg:size-[34px] lg:rounded-[10px] lg:text-sm", rail.tone)}>{rail.badge}</span>
        <div className="min-w-0">
          <p className="truncate text-sm font-semibold text-foreground lg:text-[15px]">{rail.label}</p>
          <p className="truncate text-[11px] text-muted-foreground">{rail.chain}</p>
        </div>
      </div>
      <div className="flex gap-2">
        <RailAction href={`/wallet/deposit?network=${network}`} enabled={canDeposit} className={WALLET_CTA}>
          Deposit
        </RailAction>
        <RailAction href={`/wallet/withdraw?network=${network}`} enabled={canWithdraw} className={WALLET_CTA_GHOST}>
          Withdraw
        </RailAction>
      </div>
    </div>
  );
}

// An unavailable direction stays visible but inert, so the card reads the same on every rail
// and the missing capability is legible rather than silently absent.
function RailAction({ href, enabled, className, children }: { href: string; enabled: boolean; className: string; children: string }) {
  if (!enabled) {
    return (
      <span aria-disabled className={cn(className, "flex-1 cursor-not-allowed py-2 text-xs opacity-40")} title={`${children} is not available on this rail yet`}>
        {children}
      </span>
    );
  }
  return (
    <Link href={href} className={cn(className, "flex-1 py-2 text-xs")}>
      {children}
    </Link>
  );
}
