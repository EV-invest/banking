"use client";

import { ArrowUpRight, Loader2, X } from "lucide-react";
import { Link } from "@/shared/ui/cabinet-link";
import { type CSSProperties, useState } from "react";

import { Skeleton } from "@evinvest/uikit";

import { cancelWithdrawal, depositsResource, withdrawalsResource } from "@/entities/wallet/model/wallet-resource";
import type { Deposit, Withdrawal } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { formatUsdt, networkLabel, railMeta, shortAddress } from "@/views/wallet/lib/format";
import { WALLET_CARD, WALLET_CTA, WalletScreen } from "@/views/wallet/ui/wallet-chrome";

// The desktop table track — network, destination, amount, status. Fixed px columns with no
// matching step on the spacing scale, so they ride in as a custom property rather than an
// arbitrary class, and the header row and the body rows stay locked to one definition.
const COLUMNS = { "--activity-columns": "110px 1fr 150px 130px" } as CSSProperties;

const STATUS_STYLES: Record<string, string> = {
  queued: "bg-main-accent-t3/15 text-main-accent-t3",
  processing: "bg-main-accent-t1/15 text-main-accent-t1",
  completed: "bg-main-accent-t2/15 text-main-accent-t2",
  credited: "bg-main-accent-t2/15 text-main-accent-t2",
  failed: "bg-main-accent-t4/15 text-main-accent-t4",
  cancelled: "bg-muted text-muted-foreground",
};

// One row of the merged feed. Withdrawals carry no timestamp on the wire, so a strict time
// interleave isn't possible: withdrawals keep the hub's newest-first order on top (they hold
// the actionable queued rows), then deposits newest-first.
interface Entry {
  key: string;
  id: string;
  network: string;
  title: string;
  sub: string;
  amount: string;
  state: string;
  cancellable: boolean;
}

export function ActivityView() {
  const [cancelError, setCancelError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const withdrawals = useResource(withdrawalsResource);
  const deposits = useResource(depositsResource);

  const cancel = async (id: string) => {
    setBusy(id);
    try {
      // Cancelling names the wallet, withdrawals and operations tags, so this list
      // refreshes itself — and so does the balance on any other open surface.
      await cancelWithdrawal(id);
    } catch (e) {
      setCancelError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  const loading = withdrawals.isLoading || deposits.isLoading;
  const rows = withdrawals.data?.withdrawals ?? [];
  const credits = deposits.data?.deposits ?? [];
  // A failed refresh keeps the list it last showed; only a read that never succeeded is
  // worth reporting in its place.
  const error = cancelError ?? (withdrawals.data ? null : (withdrawals.error?.message ?? null)) ?? (deposits.data ? null : (deposits.error?.message ?? null));
  const entries: Entry[] = loading ? [] : buildEntries(rows, credits);

  return (
    <WalletScreen title="Activity" subtitle="Deposits and withdrawals — queued, processing, completed" back="/wallet">
      {error && <p className="text-sm text-destructive">{error}</p>}

      {loading ? (
        <Skeleton className="h-64 w-full rounded-xl lg:max-w-190" />
      ) : entries.length === 0 ? (
        <EmptyState />
      ) : (
        <div className={cn(WALLET_CARD, "overflow-hidden lg:max-w-190")}>
          <div
            style={COLUMNS}
            className="hidden grid-cols-(--activity-columns) items-center gap-3 border-b border-border px-5 py-3.5 text-xs font-medium text-muted-foreground lg:grid"
          >
            <span>NETWORK</span>
            <span>DESTINATION</span>
            <span className="text-right">AMOUNT</span>
            <span className="text-right">STATUS</span>
          </div>
          {entries.map((entry, i) => (
            <Row key={entry.key} entry={entry} first={i === 0} busy={busy === entry.id} onCancel={() => cancel(entry.id)} />
          ))}
        </div>
      )}
    </WalletScreen>
  );
}

function Row({ entry, first, busy, onCancel }: { entry: Entry; first: boolean; busy: boolean; onCancel: () => void }) {
  const rail = railMeta(entry.network);
  return (
    <div style={COLUMNS} className={cn("flex items-center gap-3 px-3.5 py-3 lg:grid lg:grid-cols-(--activity-columns) lg:px-5 lg:py-3.5", !first && "border-t border-border")}>
      <span className="flex items-center gap-2.5 lg:gap-2">
        <span className={cn("flex size-8 shrink-0 items-center justify-center rounded-md text-xs font-semibold lg:size-6", rail.tone)}>{rail.badge}</span>
        <span className="hidden text-sm text-foreground lg:inline">{rail.label}</span>
      </span>

      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-sm font-medium text-foreground">{entry.title}</span>
        <span className="truncate text-xs text-muted-foreground">
          <span className="lg:hidden">
            {rail.label} · {entry.sub}
          </span>
          <span className="hidden lg:inline">{entry.sub}</span>
        </span>
      </span>

      <span className="flex shrink-0 flex-col items-end gap-1 lg:contents">
        <span className="text-sm font-medium tabular-nums text-foreground lg:text-right">{entry.amount}</span>
        <span className="flex items-center justify-end gap-2">
          <span className={cn("rounded-full px-2 py-0.5 text-xs font-medium capitalize", STATUS_STYLES[entry.state] ?? "bg-muted text-muted-foreground")}>{entry.state}</span>
          {entry.cancellable && (
            <button
              type="button"
              disabled={busy}
              onClick={onCancel}
              aria-label="Cancel withdrawal"
              className="rounded-md border border-border p-1 text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            >
              {busy ? <Loader2 className="size-3 animate-spin" /> : <X className="size-3" />}
            </button>
          )}
        </span>
      </span>
    </div>
  );
}

function EmptyState() {
  return (
    <div className={cn(WALLET_CARD, "flex flex-col items-center gap-3 px-6 py-12 text-center lg:max-w-100")}>
      <span className="flex size-11 items-center justify-center rounded-xl bg-main-accent-t1/15">
        <ArrowUpRight className="size-5 text-main-accent-t1" />
      </span>
      <p className="text-sm font-semibold text-foreground">No activity yet</p>
      <p className="max-w-65 text-xs text-muted-foreground">Your deposits and withdrawals will appear here once you make your first transfer.</p>
      <Link href="/wallet/deposit" className={cn(WALLET_CTA, "mt-1 px-4 py-2.5 text-sm")}>
        New deposit
      </Link>
    </div>
  );
}

function buildEntries(withdrawals: Withdrawal[], deposits: Deposit[]): Entry[] {
  const out: Entry[] = withdrawals.map((w, i) => ({
    key: `w-${w.id ?? i}`,
    id: w.id ?? "",
    network: w.network ?? "",
    title: shortAddress(w.address),
    sub: w.tx_ref ? `tx ${shortAddress(w.tx_ref)}` : `${networkLabel(w.network)} · USDT`,
    amount: `−${formatUsdt(w.amount)} USDT`,
    state: w.state ?? "queued",
    cancellable: w.state === "queued",
  }));

  const sorted = [...deposits].sort((a, b) => Number(b.created_at ?? 0) - Number(a.created_at ?? 0));
  for (const [i, d] of sorted.entries()) {
    out.push({
      // tx_ref alone can be missing (or repeat for multi-output txs) — suffix with
      // created_at + position so React keys stay unique.
      key: `d-${d.tx_ref ?? ""}-${d.created_at ?? ""}-${i}`,
      id: d.tx_ref ?? "",
      network: d.network ?? "",
      title: d.tx_ref ? shortAddress(d.tx_ref) : "Deposit",
      sub: `${networkLabel(d.network)} · USDT`,
      amount: `+${formatUsdt(d.amount)} USDT`,
      state: "credited",
      cancellable: false,
    });
  }
  return out;
}
