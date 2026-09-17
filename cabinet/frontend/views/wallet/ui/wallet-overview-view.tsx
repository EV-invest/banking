"use client";

import { useLocale, useT } from "@evinvest/i18n/react";

import { Waypoints } from "lucide-react";
import { Link } from "@/shared/ui/cabinet-link";
import { Alert, AlertDescription, AlertTitle, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton } from "@evinvest/uikit";

import { walletResource } from "@/entities/wallet/model/wallet-resource";
import { useKycGate, VerificationRequired } from "@/features/kyc";
import { errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { StaggerItem } from "@/shared/ui/motion";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { NetworkMark } from "@/shared/ui/icons/networks";
import { formatUsdt, railMeta } from "@/views/wallet/lib/format";
import { useFirstDepositSignal } from "@/views/wallet/model/use-first-deposit-signal";
import { FieldLabel, WALLET_CARD, WALLET_CTA, WALLET_CTA_GHOST, WalletScreen } from "@/views/wallet/ui/wallet-chrome";

// The wallet landing surface (Figma `cabinet/wallet` + `cabinet/mobile/wallet`): one balance
// up top, then the rails as cards — each rail is only a way in and out of that single balance,
// never a balance of its own. Deposit/withdraw are their own routes, so a rail card links
// straight into the right screen with the rail preselected.
export function WalletOverviewView() {
  const t = useT();
  useFirstDepositSignal();
  const locale = useLocale();
  // The same cached balance Home, Deposit, Withdraw and Invest read, so arriving here from
  // any of them shows the figure immediately and refreshes it behind the number. A failed
  // refresh reports itself without blanking what is already on screen.
  const { data: wallet, error: failure, isLoading: loading } = useResource(walletResource);
  // The tier from `/kyc/status` rather than from the profile's mirror of it, which lands a
  // poll later: a reader coming back from the vendor used to watch the banner and the profile
  // card update while this screen went on hiding rails the hub was already serving. A read
  // that FAILED still gates nothing — see `features/kyc/lib/money-gate`, where that rule now
  // lives as a predicate with tests instead of as this expression's third copy.
  const { gated, loading: tierLoading } = useKycGate();
  const error = wallet ? null : failure ? errorMessage(failure, t) : null;

  const balance = wallet?.balance;

  const depositable = new Set((wallet?.deposit_addresses ?? []).map((a) => a.network ?? "").filter(Boolean));
  const withdrawable = new Set((wallet?.withdrawable ?? []).map((w) => w.network ?? "").filter(Boolean));
  // Union of both directions — a rail watched for deposits but not yet funded for payouts
  // still belongs on the list, with its unavailable action shown as disabled.
  const rails = [...new Set([...depositable, ...withdrawable])];

  return (
    <WalletScreen
      title={t("ui.wallet")}
      subtitle={t("wallet.overviewSub")}
      actions={
        <>
          <Link href="/wallet/deposit" className={cn(WALLET_CTA, "px-4 py-2.5 text-sm")}>
            {t("ui.deposit")}
          </Link>
          <Link href="/wallet/withdraw" className={cn(WALLET_CTA_GHOST, "px-4 py-2.5 text-sm")}>
            {t("ui.withdraw")}
          </Link>
          <Link href="/invest" className={cn(WALLET_CTA_GHOST, "px-4 py-2.5 text-sm")}>
            {t("ui.allocate")}
          </Link>
          <Link href="/wallet/activity" className={cn(WALLET_CTA_GHOST, "px-4 py-2.5 text-sm")}>
            {t("ui.walletHistory")}
          </Link>
        </>
      }
    >
      {error && (
        <StaggerItem>
          {/* uikit's own destructive Alert. This screen's state matrix — loading, gated,
              empty, list — is drawn from the kit throughout; the error was the one branch
              still assembled out of a card, an icon and two paragraphs. */}
          <Alert variant="destructive">
            <AlertTitle>{t("err.walletLoad")}</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        </StaggerItem>
      )}

      <StaggerItem className={cn(WALLET_CARD, "flex flex-col gap-4 p-5 lg:flex-row lg:items-center lg:justify-between lg:gap-6 lg:px-7 lg:py-6")}>
        <div className="flex flex-col gap-2">
          <FieldLabel className="tracking-wider">{t("wallet.totalBalance")}</FieldLabel>
          <div className="flex items-baseline gap-2">
            {loading ? (
              <Skeleton className="h-9 w-44 lg:h-12 lg:w-64" />
            ) : (
              <p className="text-4xl font-semibold leading-none text-ink tabular-nums lg:text-5xl">{formatUsdt(balance?.total, locale)}</p>
            )}
            <p className="text-sm font-medium text-ink-soft lg:text-base">USDT</p>
          </div>
          {/* `wallet.balance.model` is a section-type tip (a descriptor block, not an inline ⓘ),
              so it would break this row — the chips carry the inline tips instead. */}
          <p className="hidden whitespace-nowrap text-sm text-ink-soft lg:block">{t("wallet.oneFungibleBalance", { amount: formatUsdt(balance?.total, locale) })}</p>
        </div>
        {/* Four chips, one per term of the balance identity (total = available + in orders
            + invested + pending withdrawal). Two columns at 390px, not four: four leave
            ~55px for a label, which no short form fits. The short forms date from the
            three-across layout and stay, since a 2×2 chip still shares its row with a dot
            and a tip. i18n-max: 7 on every `label`; the `wideLabel` forms only render from
            `lg`, where the chips size to their content. */}
        <div className="grid grid-cols-2 gap-2.5 lg:flex lg:shrink-0">
          <Chip label={t("wallet.chip.availShort")} wideLabel={t("wallet.chip.avail")} dot="bg-positive" value={balance?.available} loading={loading} tip="wallet.balance.available" />
          <Chip label={t("wallet.chip.inOrdersShort")} wideLabel={t("wallet.chip.inOrders")} dot="bg-chart-4" value={balance?.in_orders} loading={loading} tip="wallet.balance.in-orders" />
          <Chip label={t("wallet.chip.investShort")} wideLabel={t("wallet.chip.invest")} dot="bg-accent-warn" value={balance?.invested} loading={loading} tip="wallet.balance.invested" />
          <Chip label={t("wallet.pendWd")} wideLabel={t("wallet.chip.pendingWd")} dot="bg-accent-debug" value={balance?.pending_withdrawal} loading={loading} tip="wallet.balance.pending-withdrawal" />
        </div>
      </StaggerItem>

      {/* Three equal buttons across a 390px phone, ~113px each. i18n-max: 11 on all three. */}
      <StaggerItem className="grid grid-cols-3 gap-2 lg:hidden">
        <Link href="/wallet/deposit" className={cn(WALLET_CTA, "py-2.5 text-sm")}>
          {t("ui.deposit")}
        </Link>
        <Link href="/wallet/withdraw" className={cn(WALLET_CTA_GHOST, "py-2.5 text-sm")}>
          {t("ui.withdraw")}
        </Link>
        <Link href="/invest" className={cn(WALLET_CTA_GHOST, "py-2.5 text-sm")}>
          {t("ui.allocate")}
        </Link>
      </StaggerItem>

      <StaggerItem className="flex items-center justify-between">
        <p className="text-sm font-semibold text-ink">{t("ui.networks")}</p>
        {/* The Figma frames leave the activity screen with no entry point; this is it. */}
        <Link href="/wallet/activity" className="rounded-md text-xs text-accent-debug outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring lg:hidden">
          {t("ui.walletHistory")}
        </Link>
        <p className="hidden text-xs text-ink-soft lg:block">{t("wallet.railsCaption")}</p>
      </StaggerItem>

      {/* One item for all three branches: the rails are a single section of this screen
          whichever of them it is showing, and giving each branch its own item would make
          the sequence depend on which one happened to render. */}
      <StaggerItem>
        {loading || tierLoading ? (
          <div className="grid gap-3.5 lg:grid-cols-2 lg:gap-5 xl:grid-cols-3">
            <Skeleton className="h-27 rounded-xl" />
            <Skeleton className="hidden h-31 rounded-xl lg:block" />
            <Skeleton className="hidden h-31 rounded-xl xl:block" />
          </div>
        ) : gated ? (
          // The rails themselves, not a notice above them: below tier 1 the hub issues no
          // deposit address on ANY network and refuses every withdrawal, so every action on
          // every card can only refuse. Leaving them clickable is the wrong-cause bug of #215
          // one screen earlier — a rail named as the problem when the account is. The balance
          // above stays, deliberately: the hub shows it to an unverified caller on purpose,
          // and hiding it would say their money is gated when only the rails are.
          <VerificationRequired title={t("wallet.overviewVerifyTitle")} description={t("wallet.overviewVerifyBody")} />
        ) : rails.length === 0 ? (
          // The other zero state of this same section, and it used to be the bare grey
          // sentence the gated branch beside it was designed away from.
          <Empty className="border md:p-6">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Waypoints />
              </EmptyMedia>
              <EmptyTitle>{t("wallet.noRailsTitle")}</EmptyTitle>
              <EmptyDescription>{t("wallet.noRails")}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <div className="grid gap-3.5 lg:grid-cols-2 lg:gap-5 xl:grid-cols-3">
            {rails.map((network) => (
              <RailCard key={network} network={network} canDeposit={depositable.has(network)} canWithdraw={withdrawable.has(network)} />
            ))}
          </div>
        )}
      </StaggerItem>
    </WalletScreen>
  );
}

// The label shortens on mobile (`AVAIL`) and spells out from `lg` up — the Figma frames use
// both, and the chip is too narrow at 390px for the long form.
function Chip({ label, wideLabel, dot, value, loading, tip }: { label: string; wideLabel: string; dot: string; value: string | undefined; loading: boolean; tip: TipKey }) {
  const locale = useLocale();
  return (
    <div className="flex min-w-0 flex-col gap-1.5 rounded-lg border border-border px-2.5 py-2.5 lg:px-4 lg:py-3.5">
      <span className="flex items-center gap-1.5">
        <span className={cn("size-1.5 shrink-0 rounded-full lg:size-2", dot)} />
        <span className="truncate text-xs font-medium text-ink-soft">
          <span className="lg:hidden">{label}</span>
          <span className="hidden lg:inline">{wideLabel}</span>
        </span>
        <TipAnchor anchor={tip} />
      </span>
      {loading ? <Skeleton className="h-5 w-16 lg:h-7 lg:w-20" /> : <p className="truncate text-sm font-semibold text-ink tabular-nums lg:text-lg">{formatUsdt(value, locale)}</p>}
    </div>
  );
}

function RailCard({ network, canDeposit, canWithdraw }: { network: string; canDeposit: boolean; canWithdraw: boolean }) {
  const t = useT();
  const rail = railMeta(network);
  return (
    <div className={cn(WALLET_CARD, "flex flex-col gap-3.5 p-4.5 lg:gap-4 lg:p-5")}>
      <div className="flex items-center gap-2.5">
        <span className={cn("flex size-8 shrink-0 items-center justify-center rounded-md text-xs font-semibold lg:size-8.5 lg:rounded-lg lg:text-sm", rail.tone)}>
          <NetworkMark network={network} className="size-4.5 lg:size-5" />
        </span>
        <div className="min-w-0">
          <p className="truncate text-sm font-semibold text-ink">{rail.label}</p>
          <p className="truncate text-xs text-ink-soft">{t(rail.chainKey)}</p>
        </div>
      </div>
      <div className="flex gap-2">
        <RailAction href={`/wallet/deposit?network=${network}`} enabled={canDeposit} className={WALLET_CTA}>
          {t("ui.deposit")}
        </RailAction>
        <RailAction href={`/wallet/withdraw?network=${network}`} enabled={canWithdraw} className={WALLET_CTA_GHOST}>
          {t("ui.withdraw")}
        </RailAction>
      </div>
    </div>
  );
}

// An unavailable direction stays visible but inert, so the card reads the same on every rail
// and the missing capability is legible rather than silently absent.
function RailAction({ href, enabled, className, children }: { href: `/${string}`; enabled: boolean; className: string; children: string }) {
  const t = useT();
  if (!enabled) {
    return (
      <span aria-disabled className={cn(className, "flex-1 cursor-not-allowed py-2 text-xs opacity-40")} title={t("wallet.railActionUnavailable", { action: children })}>
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
