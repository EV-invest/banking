"use client";

import { ArrowDownToLine, ArrowUpFromLine, Check, Clock, Copy, Loader2, TriangleAlert, Wallet as WalletIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { Alert, AlertDescription, AlertTitle, Button, Card, CardContent, CardHeader, CardTitle, Input, Skeleton, Tabs, TabsContent, TabsList, TabsTrigger } from "@evinvest/uikit";

import { fetchDepositAddress, fetchWallet, fetchWithdrawals, submitWithdrawal } from "@/entities/wallet/api/wallet-client";
import type { DepositAddress, Wallet, WalletNetwork, Withdrawal } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { formatUsdt, NETWORKS, networkLabel, shortAddress } from "@/views/wallet/lib/format";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

function sum(wallet: Wallet | null, pick: (n: WalletNetwork) => string | undefined): number {
  return (wallet?.networks ?? []).reduce((acc, n) => acc + Number(pick(n) ?? 0), 0);
}

function networkOf(wallet: Wallet | null, network: string): WalletNetwork | undefined {
  return (wallet?.networks ?? []).find((n) => n.network === network);
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

  const total = useMemo(() => formatUsdt(String(sum(wallet, (n) => n.total))), [wallet]);
  const available = useMemo(() => formatUsdt(String(sum(wallet, (n) => n.available))), [wallet]);

  return (
    <div className="container max-w-5xl space-y-8 py-12">
      <header className="space-y-1">
        <p className="font-mono-tech text-xs uppercase tracking-widest text-main-accent-t1">Wallet</p>
        <h1 className="text-3xl font-semibold">Your USDT</h1>
        <p className="text-sm text-muted-foreground">Balances across BEP20, TRC20 and TON — read live from the ledger.</p>
      </header>

      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertTitle>Couldn&apos;t load your wallet</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <Card>
        <CardContent className="flex flex-wrap items-end justify-between gap-6 pt-6">
          <div className="space-y-1">
            <p className="text-xs uppercase tracking-wide text-muted-foreground">Total balance</p>
            {wallet ? <p className="text-4xl font-semibold tabular-nums">{total} USDT</p> : <Skeleton className="h-10 w-48" />}
            <p className="text-sm text-muted-foreground">{wallet ? `${available} USDT available` : "—"}</p>
          </div>
          <WalletIcon className="size-10 text-main-accent-t1/60" />
        </CardContent>
      </Card>

      <Tabs defaultValue="overview">
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="deposit">Deposit</TabsTrigger>
          <TabsTrigger value="withdraw">Withdraw</TabsTrigger>
          <TabsTrigger value="activity">Activity</TabsTrigger>
        </TabsList>

        <TabsContent value="overview" className="pt-6">
          <Overview wallet={wallet} />
        </TabsContent>
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

function Overview({ wallet }: { wallet: Wallet | null }) {
  if (!wallet) {
    return (
      <div className="grid gap-4 sm:grid-cols-3">
        {NETWORKS.map((n) => (
          <Skeleton key={n} className="h-40 w-full" />
        ))}
      </div>
    );
  }
  return (
    <div className="grid gap-4 sm:grid-cols-3">
      {NETWORKS.map((network) => {
        const slice = networkOf(wallet, network);
        return (
          <Card key={network}>
            <CardHeader>
              <CardTitle className="flex items-center justify-between text-base">
                <span>{networkLabel(network)}</span>
                <span className="font-mono-tech text-xs text-muted-foreground">USDT</span>
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <Stat label="Available" value={slice?.available} emphasis />
              <Stat label="Reserved" value={slice?.reserved} />
              <Stat label="Allocated" value={slice?.allocated} />
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}

function Stat({ label, value, emphasis }: { label: string; value: string | undefined; emphasis?: boolean }) {
  return (
    <div className="flex items-baseline justify-between">
      <span className="text-xs uppercase tracking-wide text-muted-foreground">{label}</span>
      <span className={cn("tabular-nums", emphasis ? "text-lg font-semibold" : "text-sm text-muted-foreground")}>{formatUsdt(value)}</span>
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
          <div className="flex aspect-square w-40 items-center justify-center rounded-lg border border-border bg-main-surface text-xs text-muted-foreground">QR</div>
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
          <p className="text-xs text-muted-foreground">Credited after {address?.min_confirmations ?? "several"} network confirmations.</p>
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

  const slice = networkOf(wallet, network);
  const fee = Number(slice?.withdrawal_fee ?? 0);
  const youReceive = Math.max(0, Number(amount || 0) - fee);

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

  const valid = address.trim().length > 0 && Number(amount) > 0;

  return (
    <div className="max-w-xl space-y-5">
      <NetworkPicker value={network} onChange={setNetwork} />

      {done && (
        <Alert>
          <Clock className="size-4 text-main-accent-t3" />
          <AlertTitle>Withdrawal requested</AlertTitle>
          <AlertDescription>
            {formatUsdt(done.net_amount)} USDT is on its way to {shortAddress(done.address)} — it&apos;s pending on-chain confirmation.
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
        <CardContent className="space-y-4 pt-6">
          <label className="block space-y-1.5">
            <span className="text-sm">Destination address</span>
            <Input value={address} onChange={(e) => setAddress(e.target.value)} placeholder={`${networkLabel(network)} address`} spellCheck={false} />
          </label>

          <label className="block space-y-1.5">
            <span className="flex items-center justify-between text-sm">
              <span>Amount</span>
              <button type="button" className="text-xs text-main-accent-t1 hover:underline" onClick={() => setAmount(slice?.available ?? "0")}>
                Available {formatUsdt(slice?.available)} · Max
              </button>
            </span>
            <Input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" placeholder="0.00" />
          </label>

          <div className="space-y-1 rounded-lg border border-border bg-main-surface p-3 text-sm">
            <Row label="Network fee" value={`${formatUsdt(slice?.withdrawal_fee)} USDT`} />
            <Row label="You will receive" value={`${formatUsdt(String(youReceive))} USDT`} strong />
            <p className="pt-1 text-xs text-muted-foreground">Minimum withdrawal {formatUsdt(slice?.min_withdrawal)} USDT.</p>
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
  pending: "bg-main-accent-t3/15 text-main-accent-t3",
  completed: "bg-main-accent-t2/15 text-main-accent-t2",
  failed: "bg-main-accent-t4/15 text-main-accent-t4",
};

function StatusPill({ state }: { state: string | undefined }) {
  const key = state ?? "pending";
  return <span className={cn("rounded-full px-2.5 py-0.5 text-xs font-medium capitalize", STATUS_STYLES[key] ?? "bg-muted text-muted-foreground")}>{key}</span>;
}

function ActivityPanel() {
  const [items, setItems] = useState<Withdrawal[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchWithdrawals()
      .then((list) => setItems(list.withdrawals ?? []))
      .catch((e: Error) => setError(e.message));
  }, []);

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
        {items.map((w) => (
          <div key={w.id} className="flex items-center justify-between gap-4 px-4 py-3">
            <div className="min-w-0 space-y-0.5">
              <p className="text-sm">
                <span className="font-medium">{formatUsdt(w.amount)} USDT</span> <span className="text-muted-foreground">to {shortAddress(w.address)}</span>
              </p>
              <p className="font-mono-tech text-xs text-muted-foreground">
                {networkLabel(w.network)}
                {w.tx_ref ? ` · ${shortAddress(w.tx_ref)}` : ""}
              </p>
            </div>
            <StatusPill state={w.state} />
          </div>
        ))}
      </CardContent>
    </Card>
  );
}
