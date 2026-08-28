"use client";

import { Loader2, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { Button, Card, CardContent, Input, Skeleton } from "@evinvest/uikit";

import { dispatchWithdrawal, failWithdrawal, settleWithdrawal } from "@/entities/admin/api/admin-client";
import { withdrawalQueueResource } from "@/entities/admin/model/admin-resource";
import type { WithdrawalQueueItem } from "@/shared/contracts/admin";
import { useResource } from "@/shared/lib/resource";
import { TipAnchor } from "@/shared/tips";
import { Settled, StaggerItem } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { ago, formatUsd } from "@/views/admin/lib/format";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

// Which confirm panel is open under a row: settle asks for the mined tx ref,
// fail asks for an (optional) audit reason and repeats the double-pay warning.
type Panel = { id: string; kind: "settle" | "fail" };

export function WithdrawalsView() {
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [panel, setPanel] = useState<Panel | null>(null);
  const [txRef, setTxRef] = useState("");
  const [reason, setReason] = useState("");

  // Cached, so arriving from another console screen shows the queue as it was last read and
  // refreshes it behind. `refresh()` after an action replaces the manual re-fetch, and keeps
  // any other surface reading the same queue in step.
  const read = useResource(withdrawalQueueResource);
  const queue = read.data ? (read.data.items ?? []) : null;
  const error = actionError ?? (read.data ? null : (read.error?.message ?? null));

  const run = async (id: string, fn: () => Promise<unknown>) => {
    setBusy(id);
    setActionError(null);
    try {
      await fn();
      setPanel(null);
      setTxRef("");
      setReason("");
      await read.refresh();
    } catch (e) {
      setActionError((e as Error).message);
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
    <AdminScreen className="space-y-8">
      <AdminHeader eyebrow="Administer" title="Withdrawals" subtitle="Dispatch, settle, or fail withdrawals — investors' and the fund's own payouts" />

      {error && <ResourceError message={error} />}

      <StaggerItem as="section" className="space-y-3">
        <p className="flex items-center gap-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
          Awaiting action
          {/* The count pill lands on the same step as the label it trails, so its fill and
              accent colour — not a smaller size — are what set it apart. */}
          {queue && <span className="rounded-full bg-main-accent-t3/15 px-2 py-0.5 text-xs font-semibold text-main-accent-t3">{queue.length} open</span>}
        </p>
        <Card>
          <CardContent className="p-0">
            <Settled
              loading={!queue}
              skeleton={
                <div className="p-6">
                  <Skeleton className="h-32 w-full" />
                </div>
              }
            >
              {!queue ? null : queue.length === 0 ? (
                <p className="p-8 text-center text-sm text-muted-foreground">No withdrawals are awaiting action.</p>
              ) : (
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                      <th className="px-5 py-3 font-medium">User</th>
                      <th className="px-5 py-3 font-medium">
                        <span className="flex items-center gap-1.5">
                          Destination
                          <TipAnchor anchor="admin.withdrawals.destination" />
                        </span>
                      </th>
                      <th className="px-5 py-3 font-medium">
                        <span className="flex items-center gap-1.5">
                          Gross / net
                          <TipAnchor anchor="admin.withdrawals.gross-net" />
                        </span>
                      </th>
                      <th className="px-5 py-3 font-medium">
                        <span className="flex items-center gap-1.5">
                          State
                          <TipAnchor anchor="admin.withdrawals.state" />
                        </span>
                      </th>
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
            </Settled>
          </CardContent>
        </Card>
        <p className="max-w-3xl text-xs text-muted-foreground">
          Dispatch broadcasts a queued withdrawal once its rail has liquidity. Settle records the mined transaction and releases the reservation. Fail voids and
          refunds — ONLY safe when nothing reached the chain; the hub refuses it while a broadcast record exists.
        </p>
      </StaggerItem>
    </AdminScreen>
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
          {/* A revenue payout has no investor behind it — the fund is paying its own
              earnings out. Naming that beats rendering a blank User cell, and it tells
              the operator whose money the dispatch/settle below is about to move. */}
          {item.source === "revenue" ? (
            <p className="font-medium text-main-accent-t2">Fund revenue</p>
          ) : (
            <p className="font-medium">{item.email || item.user_id.slice(0, 8)}</p>
          )}
          <p className="font-mono-tech text-xs text-muted-foreground">{item.withdrawal_id.slice(0, 8)}</p>
        </td>
        <td className="px-5 py-3">
          <p className="uppercase text-xs text-muted-foreground">{item.network}</p>
          <p className="font-mono-tech text-xs" title={item.address}>
            {shortAddr(item.address)}
          </p>
        </td>
        <td className="px-5 py-3 tabular-nums">
          <p>{formatUsd(item.amount)}</p>
          <p className="text-xs text-muted-foreground">{formatUsd(item.net_amount)} net</p>
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
        <tr className="bg-foreground/5">
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
                <TipAnchor anchor="admin.withdrawals.settle.tx-hash" />
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
