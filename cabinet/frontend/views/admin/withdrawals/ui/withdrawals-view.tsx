"use client";

import { Loader2, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Button, Card, CardContent, Input, Skeleton } from "@evinvest/uikit";

import { dispatchWithdrawal, failWithdrawal, fetchWithdrawalQueue, settleWithdrawal } from "@/entities/admin/api/admin-client";
import type { WithdrawalQueueItem } from "@/shared/contracts/admin";
import { ago, usd } from "@/views/admin/lib/format";
import { AdminHeader } from "@/views/admin/ui/shell";

// Which confirm panel is open under a row: settle asks for the mined tx ref,
// fail asks for an (optional) audit reason and repeats the double-pay warning.
type Panel = { id: string; kind: "settle" | "fail" };

export function WithdrawalsView() {
  const [queue, setQueue] = useState<WithdrawalQueueItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [panel, setPanel] = useState<Panel | null>(null);
  const [txRef, setTxRef] = useState("");
  const [reason, setReason] = useState("");

  const load = useCallback(() => {
    fetchWithdrawalQueue()
      .then((q) => setQueue(q.items ?? []))
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const run = async (id: string, fn: () => Promise<unknown>) => {
    setBusy(id);
    setError(null);
    try {
      await fn();
      setPanel(null);
      setTxRef("");
      setReason("");
      load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  const openPanel = (next: Panel) => {
    setPanel((current) => (current && current.id === next.id && current.kind === next.kind ? null : next));
    setTxRef("");
    setReason("");
  };

  return (
    <div className="space-y-8 px-8 pb-10 pt-6">
      <AdminHeader eyebrow="Administer" title="Withdrawals" subtitle="Dispatch, settle, or fail user withdrawals" />

      {error && (
        <p className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4" /> {error}
        </p>
      )}

      <section className="space-y-3">
        <p className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-widest text-muted-foreground">
          Awaiting action
          {queue && <span className="rounded-full bg-main-accent-t3/15 px-2 py-0.5 text-[10px] font-semibold text-main-accent-t3">{queue.length} open</span>}
        </p>
        <Card>
          <CardContent className="p-0">
            {!queue ? (
              <div className="p-6">
                <Skeleton className="h-32 w-full" />
              </div>
            ) : queue.length === 0 ? (
              <p className="p-8 text-center text-sm text-muted-foreground">No withdrawals are awaiting action.</p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                    <th className="px-5 py-3 font-medium">User</th>
                    <th className="px-5 py-3 font-medium">Destination</th>
                    <th className="px-5 py-3 font-medium">Gross / net</th>
                    <th className="px-5 py-3 font-medium">State</th>
                    <th className="px-5 py-3 font-medium">Age</th>
                    <th className="px-5 py-3 text-right font-medium">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {queue.map((item) => {
                    const isBusy = busy === item.withdrawal_id;
                    const open = panel?.id === item.withdrawal_id ? panel : null;
                    return (
                      <WithdrawalRow
                        key={item.withdrawal_id}
                        item={item}
                        busy={isBusy}
                        panel={open}
                        txRef={txRef}
                        reason={reason}
                        onTxRef={setTxRef}
                        onReason={setReason}
                        onOpen={openPanel}
                        onDispatch={() => run(item.withdrawal_id, () => dispatchWithdrawal(item.withdrawal_id))}
                        onSettle={() => run(item.withdrawal_id, () => settleWithdrawal(item.withdrawal_id, txRef))}
                        onFail={() => run(item.withdrawal_id, () => failWithdrawal(item.withdrawal_id, reason))}
                      />
                    );
                  })}
                </tbody>
              </table>
            )}
          </CardContent>
        </Card>
        <p className="max-w-3xl text-xs text-muted-foreground">
          Dispatch broadcasts a queued withdrawal once its rail has liquidity. Settle records the mined transaction and releases the reservation. Fail voids and
          refunds — ONLY safe when nothing reached the chain; the hub refuses it while a broadcast record exists.
        </p>
      </section>
    </div>
  );
}

function WithdrawalRow({
  item,
  busy,
  panel,
  txRef,
  reason,
  onTxRef,
  onReason,
  onOpen,
  onDispatch,
  onSettle,
  onFail,
}: {
  item: WithdrawalQueueItem;
  busy: boolean;
  panel: Panel | null;
  txRef: string;
  reason: string;
  onTxRef: (v: string) => void;
  onReason: (v: string) => void;
  onOpen: (p: Panel) => void;
  onDispatch: () => void;
  onSettle: () => void;
  onFail: () => void;
}) {
  const queued = item.state === "queued";
  return (
    <>
      <tr>
        <td className="px-5 py-3">
          <p className="font-medium">{item.email || item.user_id.slice(0, 8)}</p>
          <p className="font-mono-tech text-xs text-muted-foreground">{item.withdrawal_id.slice(0, 8)}</p>
        </td>
        <td className="px-5 py-3">
          <p className="uppercase text-xs text-muted-foreground">{item.network}</p>
          <p className="font-mono-tech text-xs" title={item.address}>
            {shortAddr(item.address)}
          </p>
        </td>
        <td className="px-5 py-3 tabular-nums">
          <p>{usd(item.amount)}</p>
          <p className="text-xs text-muted-foreground">{usd(item.net_amount)} net</p>
        </td>
        <td className="px-5 py-3">
          <span className={queued ? "text-main-accent-t3" : "text-main-accent-t2"}>{item.state}</span>
        </td>
        <td className="px-5 py-3 text-muted-foreground">{ago(item.created_at)}</td>
        <td className="px-5 py-3">
          <div className="flex justify-end gap-2">
            {queued ? (
              <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onDispatch}>
                {busy ? <Loader2 className="size-4 animate-spin" /> : null}
                Dispatch
              </Button>
            ) : (
              <>
                <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => onOpen({ id: item.withdrawal_id, kind: "settle" })}>
                  Settle
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="border-destructive/40 text-destructive hover:bg-destructive/10"
                  disabled={busy}
                  onClick={() => onOpen({ id: item.withdrawal_id, kind: "fail" })}
                >
                  Fail
                </Button>
              </>
            )}
          </div>
        </td>
      </tr>
      {panel && (
        <tr className="bg-foreground/[0.02]">
          <td colSpan={6} className="px-5 py-3">
            {panel.kind === "settle" ? (
              <div className="flex items-center gap-3">
                <Input
                  value={txRef}
                  onChange={(e) => onTxRef(e.target.value)}
                  placeholder="Mined transaction hash (0x…)"
                  spellCheck={false}
                  className="max-w-xl font-mono-tech text-xs"
                />
                <Button type="button" size="sm" disabled={busy || !txRef.trim()} onClick={onSettle}>
                  {busy ? <Loader2 className="size-4 animate-spin" /> : null}
                  Confirm settle
                </Button>
              </div>
            ) : (
              <div className="space-y-2">
                <p className="flex items-center gap-2 text-xs text-destructive">
                  <TriangleAlert className="size-4" /> Failing refunds the user. If the broadcast reached the chain this would double-pay — the hub refuses while
                  a broadcast record exists, but verify on-chain first.
                </p>
                <div className="flex items-center gap-3">
                  <Input value={reason} onChange={(e) => onReason(e.target.value)} placeholder="Reason (audit note, optional)" className="max-w-xl text-xs" />
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    className="border-destructive/40 text-destructive hover:bg-destructive/10"
                    disabled={busy}
                    onClick={onFail}
                  >
                    {busy ? <Loader2 className="size-4 animate-spin" /> : null}
                    Confirm fail
                  </Button>
                </div>
              </div>
            )}
          </td>
        </tr>
      )}
    </>
  );
}

function shortAddr(address: string): string {
  return address.length > 18 ? `${address.slice(0, 8)}…${address.slice(-6)}` : address;
}
