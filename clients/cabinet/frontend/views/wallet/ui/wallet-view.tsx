"use client";

import { ArrowDownToLine, ArrowUpFromLine, Check, Clock, Copy, Loader2, TriangleAlert, Wallet as WalletIcon, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Alert, AlertDescription, AlertTitle, Button, Card, CardContent, CardHeader, CardTitle, Input, Skeleton, Tabs, TabsContent, TabsList, TabsTrigger } from "@evinvest/uikit";

import { cancelWithdrawal, fetchDepositAddress, fetchWallet, fetchWithdrawals, submitWithdrawal } from "@/entities/wallet/api/wallet-client";
import type { DepositAddress, NetworkWithdrawable, Wallet, Withdrawal } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { DepositQr } from "@/views/wallet/ui/deposit-qr";
import { formatUsdt, fromBaseUnits, NETWORKS, networkLabel, shortAddress, subUsdt, toBaseUnits } from "@/views/wallet/lib/format";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";
// Clearly-visible active tab. The uikit default active state is `bg-background` — invisible
// on this dark theme — so the current selection reads at a glance: teal tint + teal label.
const TAB_TRIGGER = "data-[state=active]:bg-main-accent-t1/15 data-[state=active]:text-main-accent-t1 data-[state=active]:font-semibold data-[state=active]:shadow-none";

// Per-rail withdraw options for the selected network (fee, min, instant liquidity).
function withdrawableFor(wallet: Wallet | null, network: string): NetworkWithdrawable | undefined {
  return (wallet?.withdrawable ?? []).find((w) => w.network === network);
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
          <DepositPanel />
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

function NetworkPicker({ value, onChange }: { value: string; onChange: (n: string) => void }) {
  return (
    <div className="inline-flex rounded-lg border border-border p-1">
      {NETWORKS.map((network) => (
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

function DepositPanel() {
  const [network, setNetwork] = useState<string>(NETWORKS[0]);
  const [address, setAddress] = useState<DepositAddress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
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
    setNetwork(next);
    setAddress(null);
    setError(null);
    setCopied(false);
  };

  const copy = () => {
    if (!address?.address) return;
    void navigator.clipboard.writeText(address.address);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="max-w-xl space-y-5">
      <NetworkPicker value={network} onChange={selectNetwork} />
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Your {networkLabel(network)} deposit address</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {error && <p className="text-sm text-destructive">{error}</p>}
          <div className="flex justify-center">{address ? <DepositQr value={address.address ?? ""} /> : <Skeleton className="size-44 rounded-2xl" />}</div>
          <div className="flex items-center gap-2">
            {address ? (
              <code className="flex-1 break-all rounded-md border border-border bg-main-surface px-3 py-2 text-sm">{address.address}</code>
            ) : (
              <Skeleton className="h-10 flex-1" />
            )}
            <Button type="button" variant="outline" size="icon" onClick={copy} disabled={!address} aria-label="Copy address">
              {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">Credited to your one balance after {address?.min_confirmations ?? "several"} network confirmations.</p>
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

function WithdrawPanel({ wallet, onDone }: { wallet: Wallet | null; onDone: () => void }) {
  const [network, setNetwork] = useState<string>(NETWORKS[0]);
  const [address, setAddress] = useState("");
  const [amount, setAmount] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Withdrawal | null>(null);

  const opts = withdrawableFor(wallet, network);
  // Exact base-unit math on the USDT strings — no float error on 18-dp amounts.
  const amountUnits = toBaseUnits(amount);
  const youReceive = subUsdt(amount, opts?.withdrawal_fee); // decimal string
  const queuedUnits = amountUnits - toBaseUnits(opts?.instant); // > 0 ⇒ partly queued

  const submit = async () => {
    setSubmitting(true);
    setError(null);
    setDone(null);
    try {
      const withdrawal = await submitWithdrawal({ network, address, amount });
      setDone(withdrawal);
      setAddress("");
      setAmount("");
      onDone();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  };

  const valid = address.trim().length > 0 && amountUnits > 0n;

  return (
    <div className="max-w-xl space-y-5">
      <NetworkPicker value={network} onChange={setNetwork} />

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
            <Input value={address} onChange={(e) => setAddress(e.target.value)} placeholder={`${networkLabel(network)} address`} spellCheck={false} />
          </label>

          <label className="block space-y-1.5">
            <span className="flex items-center justify-between text-sm">
              <span>Amount</span>
              <button type="button" className="text-xs text-main-accent-t1 hover:underline" onClick={() => setAmount(opts?.withdrawable ?? "0")}>
                Available {formatUsdt(opts?.withdrawable)} · Max
              </button>
            </span>
            <Input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" placeholder="0.00" />
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
              Minimum {formatUsdt(opts?.min_withdrawal)} USDT · instant on {networkLabel(network)}: {formatUsdt(opts?.instant)} USDT.
            </p>
          </div>

          <Button type="button" className={cn("w-full", TEAL_CTA)} disabled={!valid || submitting} onClick={submit}>
            {submitting ? <Loader2 className="size-4 animate-spin" /> : <ArrowUpFromLine className="size-4" />}
            Review &amp; withdraw
          </Button>
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
  failed: "bg-main-accent-t4/15 text-main-accent-t4",
  cancelled: "bg-muted text-muted-foreground",
};

function StatusPill({ state }: { state: string | undefined }) {
  const key = state ?? "queued";
  return <span className={cn("rounded-full px-2.5 py-0.5 text-xs font-medium capitalize", STATUS_STYLES[key] ?? "bg-muted text-muted-foreground")}>{key}</span>;
}

function ActivityPanel() {
  const [items, setItems] = useState<Withdrawal[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(() => {
    fetchWithdrawals()
      .then((list) => setItems(list.withdrawals ?? []))
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
  if (!items) return <Skeleton className="h-40 w-full" />;
  if (items.length === 0) {
    return (
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center text-muted-foreground">
          <ArrowDownToLine className="size-6" />
          <p className="text-sm">No withdrawals yet.</p>
        </CardContent>
      </Card>
    );
  }
  return (
    <Card>
      <CardContent className="divide-y divide-border p-0">
        {items.map((w) => {
          const id = w.id ?? "";
          return (
            <div key={id} className="flex items-center justify-between gap-4 px-4 py-3">
              <div className="min-w-0 space-y-0.5">
                <p className="text-sm">
                  <span className="font-medium">{formatUsdt(w.amount)} USDT</span> <span className="text-muted-foreground">to {shortAddress(w.address)}</span>
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
      </CardContent>
    </Card>
  );
}
