"use client";

import { ArrowDownToLine, ArrowUpFromLine, Check, Clock, Copy, Loader2, TriangleAlert, Wallet as WalletIcon, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Alert, AlertDescription, AlertTitle, Button, Card, CardContent, CardHeader, CardTitle, Input, Skeleton, Tabs, TabsContent, TabsList, TabsTrigger } from "@evinvest/uikit";

import { cancelWithdrawal, fetchDepositAddress, fetchDeposits, fetchWallet, fetchWithdrawals, submitWithdrawal } from "@/entities/wallet/api/wallet-client";
import type { Deposit, DepositAddress, NetworkWithdrawable, Wallet, Withdrawal } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { tonFriendlyAddress } from "@/shared/lib/ton-address";
import { DepositQr } from "@/views/wallet/ui/deposit-qr";
import { formatUsdt, fromBaseUnits, networkLabel, shortAddress, subUsdt, toBaseUnits } from "@/views/wallet/lib/format";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";
// Clearly-visible active tab. The uikit default active state is `bg-background` — invisible
// on this dark theme — so the current selection reads at a glance: teal tint + teal label.
const TAB_TRIGGER = "data-[state=active]:bg-main-accent-t1/15 data-[state=active]:text-main-accent-t1 data-[state=active]:font-semibold data-[state=active]:shadow-none";

// Per-rail withdraw options for the selected network (fee, min, instant liquidity).
function withdrawableFor(wallet: Wallet | null, network: string): NetworkWithdrawable | undefined {
  return (wallet?.withdrawable ?? []).find((w) => w.network === network);
}

// The rails on offer come from the wallet response — the hub serves only rails with a
// running on-chain watcher, so an unconfigured network never shows a dead tab here.
function networksOf(entries: { network?: string }[] | undefined): string[] {
  return (entries ?? []).map((e) => e.network ?? "").filter(Boolean);
}

export function WalletView() {
  const [wallet, setWallet] = useState<Wallet | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    fetchWallet()
      .then((w) => {
        setWallet(w);
        setError(null);
      })
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(load, [load]);

  const balance = wallet?.balance;

  return (
    <div className="space-y-8 px-8 pb-10 pt-6">
      <header className="space-y-1">
        <p className="font-mono-tech text-xs uppercase tracking-widest text-main-accent-t1">Wallet</p>
        <h1 className="font-sans text-2xl font-semibold text-foreground">Your USDT</h1>
        <p className="text-sm text-muted-foreground">One balance — networks are just how you deposit and withdraw.</p>
      </header>

      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertTitle>Couldn&apos;t load your wallet</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <Card>
        <CardContent className="space-y-6">
          <div className="flex flex-wrap items-end justify-between gap-6">
            <div className="space-y-1">
              <p className="text-xs uppercase tracking-wide text-muted-foreground">Total balance</p>
              {balance ? <p className="text-4xl font-semibold tabular-nums">{formatUsdt(balance.total)} USDT</p> : <Skeleton className="h-10 w-48" />}
            </div>
            <WalletIcon className="size-10 text-main-accent-t1/60" />
          </div>
          <div className="grid gap-4 sm:grid-cols-3">
            <Stat label="Available" value={balance?.available} loading={!balance} emphasis />
            <Stat label="Invested" value={balance?.invested} loading={!balance} hint="Held in fund units, valued at the current NAV." />
            <Stat label="Pending withdrawal" value={balance?.pending_withdrawal} loading={!balance} />
          </div>
        </CardContent>
      </Card>

      <Tabs defaultValue="deposit">
        <TabsList>
          <TabsTrigger value="deposit" className={TAB_TRIGGER}>
            Deposit
          </TabsTrigger>
          <TabsTrigger value="withdraw" className={TAB_TRIGGER}>
            Withdraw
          </TabsTrigger>
          <TabsTrigger value="activity" className={TAB_TRIGGER}>
            Activity
          </TabsTrigger>
        </TabsList>

        <TabsContent value="deposit" className="pt-6">
          <DepositPanel wallet={wallet} />
        </TabsContent>
        <TabsContent value="withdraw" className="pt-6">
          <WithdrawPanel wallet={wallet} onDone={load} />
        </TabsContent>
        <TabsContent value="activity" className="pt-6">
          <ActivityPanel />
        </TabsContent>
      </Tabs>
    </div>
  );
}

function Stat({ label, value, loading, emphasis, hint }: { label: string; value: string | undefined; loading?: boolean; emphasis?: boolean; hint?: string }) {
  return (
    <div className="rounded-lg border border-border bg-main-surface p-4" title={hint}>
      <p className="text-xs uppercase tracking-wide text-muted-foreground">{label}</p>
      {loading ? (
        <Skeleton className="mt-1 h-7 w-24" />
      ) : (
        <p className={cn("tabular-nums", emphasis ? "text-2xl font-semibold" : "text-lg")}>
          {formatUsdt(value)} <span className="text-xs text-muted-foreground">USDT</span>
        </p>
      )}
    </div>
  );
}

function NetworkPicker({ networks, value, onChange }: { networks: string[]; value: string; onChange: (n: string) => void }) {
  return (
    <div className="inline-flex rounded-lg border border-border p-1">
      {networks.map((network) => (
        <button
          key={network}
          type="button"
          onClick={() => onChange(network)}
          className={cn("rounded-md px-4 py-1.5 text-sm transition-colors", value === network ? "bg-main-accent-t1 text-main-black" : "text-muted-foreground hover:text-foreground")}
        >
          {networkLabel(network)}
        </button>
      ))}
    </div>
  );
}

function PanelSkeleton() {
  return (
    <div className="max-w-xl space-y-5">
      <Skeleton className="h-10 w-60" />
      <Skeleton className="h-64 w-full" />
    </div>
  );
}

function DepositPanel({ wallet }: { wallet: Wallet | null }) {
  const networks = networksOf(wallet?.deposit_addresses);
  const [selected, setSelected] = useState<string | null>(null);
  const network = selected ?? networks[0];
  const [address, setAddress] = useState<DepositAddress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

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
  const displayAddress = network === "ton" ? (tonFriendlyAddress(rawAddress) ?? rawAddress) : rawAddress;

  const copy = () => {
    if (!displayAddress) return;
    void navigator.clipboard.writeText(displayAddress);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  if (wallet === null) return <PanelSkeleton />;
  if (!network) return <p className="text-sm text-muted-foreground">No deposit rails are available right now — check back soon.</p>;

  return (
    <div className="max-w-xl space-y-5">
      <NetworkPicker networks={networks} value={network} onChange={selectNetwork} />
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Your {networkLabel(network)} deposit address</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {error && <p className="text-sm text-destructive">{error}</p>}
          {address === null ? (
            <>
              <div className="flex justify-center">
                <Skeleton className="size-44 rounded-2xl" />
              </div>
              <Skeleton className="h-10 w-full" />
            </>
          ) : displayAddress ? (
            <>
              <div className="flex justify-center">
                <DepositQr value={displayAddress} />
              </div>
              <div className="flex items-center gap-2">
                <code className="flex-1 break-all rounded-md border border-border bg-main-surface px-3 py-2 text-sm">{displayAddress}</code>
                <Button type="button" variant="outline" size="icon" onClick={copy} aria-label="Copy address">
                  {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">Credited to your one balance after {address.min_confirmations} network confirmations.</p>
            </>
          ) : (
            <p className="text-sm text-muted-foreground">
              A {networkLabel(network)} deposit address isn&apos;t available yet — this rail is still being provisioned. Check back soon.
            </p>
          )}
        </CardContent>
      </Card>
      <Alert>
        <TriangleAlert className="size-4 text-main-accent-t3" />
        <AlertTitle>Send only USDT on {networkLabel(network)}</AlertTitle>
        <AlertDescription>Sending any other asset or using a different network will lose the funds permanently.</AlertDescription>
      </Alert>
    </div>
  );
}

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

function WithdrawPanel({ wallet, onDone }: { wallet: Wallet | null; onDone: () => void }) {
  const networks = networksOf(wallet?.withdrawable);
  const [selected, setSelected] = useState<string | null>(null);
  const network = selected ?? networks[0] ?? "";
  const [address, setAddress] = useState("");
  const [amount, setAmount] = useState("");
  const [confirming, setConfirming] = useState<ReviewedWithdrawal | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Withdrawal | null>(null);

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
      onDone();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
      setConfirming(null);
    }
  };

  const valid = network !== "" && address.trim().length > 0 && amountUnits > 0n;

  if (wallet === null) return <PanelSkeleton />;
  if (networks.length === 0) return <p className="text-sm text-muted-foreground">No withdrawal rails are available right now — check back soon.</p>;

  return (
    <div className="max-w-xl space-y-5">
      <NetworkPicker
        networks={networks}
        value={network}
        onChange={(n) => {
          setSelected(n);
          setConfirming(null);
        }}
      />

      {done && (
        <Alert>
          <Clock className="size-4 text-main-accent-t3" />
          <AlertTitle>{done.state === "queued" ? "Withdrawal queued" : "Withdrawal submitted"}</AlertTitle>
          <AlertDescription>
            {done.state === "queued"
              ? `${formatUsdt(done.net_amount)} USDT to ${shortAddress(done.address)} is queued — it ships once the ${networkLabel(done.network)} rail is topped up.`
              : `${formatUsdt(done.net_amount)} USDT is on its way to ${shortAddress(done.address)} — pending on-chain confirmation.`}
          </AlertDescription>
        </Alert>
      )}
      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertTitle>Withdrawal failed</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <Card>
        <CardContent className="space-y-4">
          <label className="block space-y-1.5">
            <span className="text-sm">Destination address</span>
            <Input
              value={address}
              onChange={(e) => {
                setAddress(e.target.value);
                setConfirming(null);
              }}
              placeholder={`${networkLabel(network)} address`}
              spellCheck={false}
            />
          </label>

          <label className="block space-y-1.5">
            <span className="flex items-center justify-between text-sm">
              <span>Amount</span>
              <button
                type="button"
                className="text-xs text-main-accent-t1 hover:underline"
                onClick={() => {
                  setAmount(opts?.withdrawable ?? "0");
                  setConfirming(null);
                }}
              >
                Available {formatUsdt(opts?.withdrawable)} · Max
              </button>
            </span>
            <Input
              value={amount}
              onChange={(e) => {
                setAmount(e.target.value);
                setConfirming(null);
              }}
              inputMode="decimal"
              placeholder="0.00"
            />
          </label>

          <div className="space-y-1 rounded-lg border border-border bg-main-surface p-3 text-sm">
            <Row label="Network fee" value={`${formatUsdt(opts?.withdrawal_fee)} USDT`} />
            <Row label="You will receive" value={`${formatUsdt(youReceive)} USDT`} strong />
            {queuedUnits > 0n && amountUnits > 0n && (
              <p className="pt-1 text-xs text-main-accent-t3">
                ~{formatUsdt(fromBaseUnits(queuedUnits))} USDT exceeds instant {networkLabel(network)} liquidity and will be queued until the rail is topped up.
              </p>
            )}
            <p className="pt-1 text-xs text-muted-foreground">
              Minimum {formatUsdt(opts?.min_withdrawal)} USDT. Up to {formatUsdt(opts?.instant)} USDT pays out instantly on {networkLabel(network)}; anything above that is
              queued until the rail is topped up.
            </p>
          </div>

          {!confirming ? (
            <Button
              type="button"
              className={cn("w-full", TEAL_CTA)}
              disabled={!valid || submitting}
              onClick={() => setConfirming({ network, address, amount, fee: opts?.withdrawal_fee, instant: opts?.instant, rails })}
            >
              <ArrowUpFromLine className="size-4" />
              Review withdrawal
            </Button>
          ) : (
            <div className="space-y-3 rounded-lg border border-main-accent-t1/40 bg-main-accent-t1/[0.06] p-4">
              <p className="text-sm font-semibold">Confirm withdrawal</p>
              <div className="space-y-1 text-sm">
                <Row label="Network" value={networkLabel(confirming.network)} />
                <Row label="Amount" value={`${formatUsdt(confirming.amount)} USDT`} />
                <Row label="Network fee" value={`${formatUsdt(confirming.fee)} USDT`} />
                <Row label="You will receive" value={`${formatUsdt(subUsdt(confirming.amount, confirming.fee))} USDT`} strong />
              </div>
              <p className="break-all font-mono-tech text-xs text-muted-foreground">To {confirming.address}</p>
              {toBaseUnits(confirming.amount) - toBaseUnits(confirming.instant) > 0n && (
                <p className="text-xs text-main-accent-t3">
                  ~{formatUsdt(fromBaseUnits(toBaseUnits(confirming.amount) - toBaseUnits(confirming.instant)))} USDT exceeds instant {networkLabel(confirming.network)}{" "}
                  liquidity and will be queued until the rail is topped up.
                </p>
              )}
              <div className="flex gap-2">
                <Button type="button" className={cn("flex-1", TEAL_CTA)} disabled={submitting} onClick={submit}>
                  {submitting ? <Loader2 className="size-4 animate-spin" /> : <ArrowUpFromLine className="size-4" />}
                  Confirm withdrawal
                </Button>
                <Button type="button" variant="outline" disabled={submitting} onClick={() => setConfirming(null)}>
                  Back
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function Row({ label, value, strong }: { label: string; value: string; strong?: boolean }) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-muted-foreground">{label}</span>
      <span className={cn("tabular-nums", strong && "font-semibold")}>{value}</span>
    </div>
  );
}

const STATUS_STYLES: Record<string, string> = {
  queued: "bg-main-accent-t3/15 text-main-accent-t3",
  processing: "bg-main-accent-t1/15 text-main-accent-t1",
  completed: "bg-main-accent-t2/15 text-main-accent-t2",
  credited: "bg-main-accent-t2/15 text-main-accent-t2",
  failed: "bg-main-accent-t4/15 text-main-accent-t4",
  cancelled: "bg-muted text-muted-foreground",
};

function StatusPill({ state }: { state: string | undefined }) {
  const key = state ?? "queued";
  return <span className={cn("rounded-full px-2.5 py-0.5 text-xs font-medium capitalize", STATUS_STYLES[key] ?? "bg-muted text-muted-foreground")}>{key}</span>;
}

function depositDate(unixSecs: string | number | undefined): string {
  const t = Number(unixSecs ?? 0);
  if (!t) return "";
  return new Date(t * 1000).toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

function ActivityPanel() {
  const [withdrawals, setWithdrawals] = useState<Withdrawal[] | null>(null);
  const [deposits, setDeposits] = useState<Deposit[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(() => {
    Promise.all([fetchWithdrawals(), fetchDeposits()])
      .then(([w, d]) => {
        setWithdrawals(w.withdrawals ?? []);
        setDeposits(d.deposits ?? []);
        setError(null);
      })
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(load, [load]);

  const cancel = async (id: string) => {
    setBusy(id);
    try {
      await cancelWithdrawal(id);
      load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  if (error) return <p className="text-sm text-destructive">{error}</p>;
  if (!withdrawals || !deposits) return <Skeleton className="h-40 w-full" />;

  // One merged feed. Withdrawals carry no timestamp on the wire, so a strict time
  // interleave isn't possible: withdrawals keep the hub's newest-first order on top
  // (they hold the actionable queued/processing rows), then deposits newest-first.
  const sortedDeposits = [...deposits].sort((a, b) => Number(b.created_at ?? 0) - Number(a.created_at ?? 0));

  if (withdrawals.length === 0 && deposits.length === 0) {
    return (
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center text-muted-foreground">
          <ArrowDownToLine className="size-6" />
          <p className="text-sm">No deposits or withdrawals yet.</p>
        </CardContent>
      </Card>
    );
  }
  return (
    <Card>
      <CardContent className="divide-y divide-border p-0">
        {withdrawals.map((w) => {
          const id = w.id ?? "";
          return (
            <div key={`w-${id}`} className="flex items-center justify-between gap-4 px-4 py-3">
              <div className="min-w-0 space-y-0.5">
                <p className="text-sm">
                  <span className="font-medium">−{formatUsdt(w.amount)} USDT</span> <span className="text-muted-foreground">to {shortAddress(w.address)}</span>
                </p>
                <p className="font-mono-tech text-xs text-muted-foreground">
                  {networkLabel(w.network)}
                  {w.tx_ref ? ` · ${shortAddress(w.tx_ref)}` : ""}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <StatusPill state={w.state} />
                {w.state === "queued" && (
                  <Button type="button" variant="outline" size="sm" disabled={busy === id} onClick={() => cancel(id)}>
                    {busy === id ? <Loader2 className="size-3 animate-spin" /> : <X className="size-3" />}
                    Cancel
                  </Button>
                )}
              </div>
            </div>
          );
        })}
        {sortedDeposits.map((d, i) => (
          // tx_ref alone can be missing (or repeat for multi-output txs) — suffix with
          // created_at + position so React keys stay unique.
          <div key={`d-${d.tx_ref ?? ""}-${d.created_at ?? ""}-${i}`} className="flex items-center justify-between gap-4 px-4 py-3">
            <div className="min-w-0 space-y-0.5">
              <p className="text-sm">
                <span className="font-medium">+{formatUsdt(d.amount)} USDT</span> <span className="text-muted-foreground">deposit</span>
              </p>
              <p className="font-mono-tech text-xs text-muted-foreground">
                {networkLabel(d.network)}
                {d.tx_ref ? ` · ${shortAddress(d.tx_ref)}` : ""}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <span className="text-xs text-muted-foreground">{depositDate(d.created_at)}</span>
              <StatusPill state="credited" />
            </div>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}
