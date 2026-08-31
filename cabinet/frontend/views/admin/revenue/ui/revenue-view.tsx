"use client";

// Admin console — the fund's OWN money: what it earned, and paying it out on-chain.
//
// The screen's whole job is to make one distinction unmistakable, because it is the one
// an operator could otherwise get wrong with real consequences: this page moves company
// revenue (retained withdrawal fees + the settled 2-and-20), never client balances and
// never the fund's seed capital. Those are separate ledger claims that this surface
// cannot reach at all — the cap below is the money plane's, enforced against the revenue
// claim's available balance with TigerBeetle's non-negative flag underneath. The form's
// own cap is a courtesy that stops a typo early, not the control.

import { Loader2 } from "lucide-react";
import { useMemo, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, Empty, EmptyDescription, EmptyTitle, Input, Skeleton } from "@evinvest/uikit";

import { cancelRevenuePayout, requestRevenuePayout } from "@/entities/admin/api/admin-client";
import { fundRevenueResource, revenuePayoutsResource } from "@/entities/admin/model/admin-resource";
import type { RevenuePayout, RevenueRail } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { revalidateTag } from "@/shared/lib/resource";
import { useResource } from "@/shared/lib/resource";
import { Settled, StaggerItem } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { amount as formatAmount, formatUsd, railLabel, stateLabel } from "@/views/admin/lib/format";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

/** In flight — the operator can still act on these; the rest are history. */
const OPEN_STATES = new Set(["queued", "processing"]);

export function RevenueView() {
  const t = useT();
  const revenue = useResource(fundRevenueResource);
  const payouts = useResource(revenuePayoutsResource);

  const [network, setNetwork] = useState<string>("");
  const [address, setAddress] = useState("");
  const [amount, setAmount] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  const data = revenue.data ?? null;
  const rails = data?.rails ?? [];
  // The first configured rail is the default, so the form is usable without a choice.
  const rail = rails.find((r) => r.network === network) ?? rails[0];
  const history = payouts.data?.withdrawals ?? null;
  const error = actionError ?? (data || !revenue.error ? null : errorMessage(revenue.error, t));

  const available = Number(data?.available ?? "0");
  const requested = Number(amount);
  const minimum = Number(rail?.minimum ?? "0");
  // Mirrors the hub's own refusals so the operator hears about a bad amount before
  // spending a round trip on it. The hub still decides.
  const amountProblem = useMemo(() => {
    if (!amount.trim()) return null;
    if (!Number.isFinite(requested) || requested <= 0) return t("admin.revenue.err.enterAmount");
    if (rail && requested < minimum) return t("admin.revenue.err.belowMinimum", { min: formatUsd(rail.minimum) });
    if (requested > available) return t("admin.revenue.err.overAvailable", { available: formatUsd(data?.available) });
    return null;
  }, [amount, requested, rail, minimum, available, data?.available, t]);

  const canSubmit = Boolean(rail) && address.trim().length > 0 && amount.trim().length > 0 && !amountProblem && busy === null;
  // Beyond `instant` the payout is accepted and queued until the treasury is topped up —
  // say so before the click, not after, so a queued row is never read as a failure.
  const willQueue = Boolean(rail) && Number.isFinite(requested) && requested > Number(rail?.instant ?? "0");

  // A payout debits a claim and joins the operator withdrawal queue, so it moves three
  // facts, not one. Naming all three keeps the treasury and queue screens in step.
  const settle = async () => {
    revalidateTag(TAG.adminRevenue, TAG.adminQueue, TAG.adminTreasury);
    await Promise.all([revenue.refresh(), payouts.refresh()]);
  };

  const submit = async () => {
    if (!rail) return;
    setBusy("request");
    setActionError(null);
    try {
      await requestRevenuePayout({ network: rail.network, address: address.trim(), amount: amount.trim() });
      setAddress("");
      setAmount("");
      setConfirming(false);
      await settle();
    } catch (e) {
      setActionError(errorMessage(e, t));
    } finally {
      setBusy(null);
    }
  };

  const cancel = async (id: string) => {
    setBusy(id);
    setActionError(null);
    try {
      await cancelRevenuePayout(id);
      await settle();
    } catch (e) {
      setActionError(errorMessage(e, t));
    } finally {
      setBusy(null);
    }
  };

  return (
    <AdminScreen className="space-y-8">
      <AdminHeader eyebrow={t("admin.eyebrow.administer")} title={t("nav.revenue")} subtitle={t("admin.revenue.subtitle")} />

      {error && <ResourceError message={error} />}

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{t("admin.revenue.earned")}</p>
        <div className="grid gap-4 sm:grid-cols-3">
          <MoneyCard label={t("admin.revenue.earnedTotal")} value={data?.earned} hint={t("admin.revenue.earnedTotalHint")} loading={!data} />
          <MoneyCard label={t("admin.revenue.availableToPayOut")} value={data?.available} hint={t("admin.revenue.availableHint")} loading={!data} emphasis />
          <MoneyCard label={t("admin.revenue.pendingPayout")} value={data?.pending_payout} hint={t("admin.revenue.pendingHint")} loading={!data} />
        </div>
        <p className="max-w-3xl text-xs text-muted-foreground">{t("admin.revenue.ownMoneyNote")}</p>
      </StaggerItem>

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{t("admin.revenue.payOut")}</p>
        <Card>
          <CardContent className="py-5">
            {/* The spacing lives on `Settled`, not on CardContent: Settled wraps its children in a
                div of its own, so a `space-y` above it has exactly one child to act on and never
                reaches the rows inside. */}
            <Settled className="space-y-4" loading={!data} skeleton={<Skeleton className="h-40 w-full" />}>
              {!data ? null : rails.length === 0 ? (
                <Empty>
                  <EmptyTitle>{t("admin.revenue.noRail")}</EmptyTitle>
                  <EmptyDescription>{t("admin.revenue.noRailHint")}</EmptyDescription>
                </Empty>
              ) : (
                <>
                  <div className="space-y-2">
                    <p className="text-xs text-muted-foreground">{t("admin.rail")}</p>
                    <div className="flex flex-wrap gap-2">
                      {rails.map((r) => (
                        <RailChip key={r.network} rail={r} selected={r.network === rail?.network} onSelect={() => setNetwork(r.network)} />
                      ))}
                    </div>
                  </div>

                  <div className="grid gap-4 sm:grid-cols-2">
                    <label className="block space-y-1.5">
                      <span className="block text-xs text-muted-foreground">{t("admin.revenue.destinationAddress")}</span>
                      <Input
                        value={address}
                        onChange={(e) => {
                          setAddress(e.target.value);
                          setConfirming(false);
                        }}
                        placeholder={t("admin.revenue.placeholder.address")}
                        spellCheck={false}
                        className="font-mono-tech text-xs"
                      />
                    </label>
                    <label className="block space-y-1.5">
                      <span className="block text-xs text-muted-foreground">{t("admin.revenue.amountUsdt")}</span>
                      <Input
                        value={amount}
                        onChange={(e) => {
                          setAmount(e.target.value);
                          setConfirming(false);
                        }}
                        inputMode="decimal"
                        placeholder={rail ? t("admin.revenue.placeholder.minAmount", { n: formatAmount(rail.minimum) }) : "0.00"}
                        className="tabular-nums"
                      />
                    </label>
                  </div>

                  {amountProblem && <p className="text-xs text-destructive">{amountProblem}</p>}
                  {!amountProblem && willQueue && (
                    <p className="text-xs text-main-accent-t3">{t("admin.revenue.willQueue", { instant: formatUsd(rail?.instant) })}</p>
                  )}

                  {/* A second, deliberate click before company money leaves. It restates the
                      destination and the amount, because those are the two things a typo
                      ruins and the chain will not give back. */}
                  {confirming ? (
                    <div className="space-y-2 rounded-lg border border-main-accent-t3/40 bg-main-accent-t3/5 p-3">
                      {/* One sentence, one key: which order the amount, the rail and the
                          address fall in is a per-language decision, so the three are ICU
                          arguments rather than spans the sentence is cut around. The address
                          is 40-plus unbroken characters, so `break-words` stays on the
                          paragraph — without it the panel widens past the phone. */}
                      <p className="text-sm break-words">
                        {t("admin.revenue.confirmSentence", {
                          amount: formatUsd(amount),
                          network: (rail?.network ?? "").toUpperCase(),
                          address: address.trim(),
                        })}
                      </p>
                      <p className="text-xs text-muted-foreground">{t("admin.revenue.confirmNote")}</p>
                      {/* i18n-max: 12 per verb — two `shrink-0` Buttons in a ≤400px panel. */}
                      <div className="flex gap-2">
                        <Button type="button" size="sm" disabled={busy !== null} onClick={submit}>
                          {busy === "request" ? <Loader2 className="size-4 animate-spin" /> : null}
                          {t("admin.revenue.confirmPayout")}
                        </Button>
                        <Button type="button" size="sm" variant="outline" disabled={busy !== null} onClick={() => setConfirming(false)}>
                          {t("ui.back")}
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <Button type="button" size="sm" disabled={!canSubmit} onClick={() => setConfirming(true)}>
                      {t("admin.revenue.reviewPayout")}
                    </Button>
                  )}
                </>
              )}
            </Settled>
          </CardContent>
        </Card>
      </StaggerItem>

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{t("admin.revenue.payouts")}</p>
        <Card>
          <CardContent className="p-0">
            <Settled
              loading={!history}
              skeleton={
                <div className="p-6">
                  <Skeleton className="h-24 w-full" />
                </div>
              }
            >
              {!history ? null : history.length === 0 ? (
                <div className="p-8">
                  <Empty>
                    <EmptyTitle>{t("admin.revenue.noPayouts")}</EmptyTitle>
                    <EmptyDescription>{t("admin.revenue.noPayoutsHint")}</EmptyDescription>
                  </Empty>
                </div>
              ) : (
                // Five columns do not fit a phone. The table scrolls inside its own box rather
                // than making the whole page scroll sideways.
                <div className="overflow-x-auto">
                  <table className="w-full min-w-140 text-sm">
                    <thead>
                      {/* i18n-max: 14 per header — the table scrolls inside `min-w-140`, so a
                          long header widens the scroll rather than clipping a cell. */}
                      <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                        <th className="px-5 py-3 font-medium">{t("ui.destination")}</th>
                        <th className="px-5 py-3 font-medium">{t("ui.amount")}</th>
                        <th className="px-5 py-3 font-medium">{t("admin.col.state")}</th>
                        <th className="px-5 py-3 font-medium">{t("admin.col.transaction")}</th>
                        <th className="px-5 py-3 text-right font-medium">{t("admin.col.actions")}</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-border">
                      {history.map((payout) => (
                        <PayoutRow key={payout.id} payout={payout} busy={busy === payout.id} onCancel={() => cancel(payout.id)} />
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </Settled>
          </CardContent>
        </Card>
        <p className="max-w-3xl text-xs text-muted-foreground">{t("admin.revenue.footnote")}</p>
      </StaggerItem>
    </AdminScreen>
  );
}

function MoneyCard({ label, value, hint, loading, emphasis }: { label: string; value: string | undefined; hint: string; loading: boolean; emphasis?: boolean }) {
  return (
    <Card>
      <CardContent className="space-y-1 py-5">
        <p className="text-xs text-muted-foreground">{label}</p>
        {loading ? (
          <Skeleton className="mt-1 h-8 w-28" />
        ) : (
          // One step for every figure; the payable one carries the difference in colour,
          // not in size, so the row keeps a single baseline.
          <p className={emphasis ? "text-3xl font-semibold tabular-nums text-main-accent-t2" : "text-3xl font-semibold tabular-nums"}>{formatUsd(value)}</p>
        )}
        {!loading && <p className="text-xs text-main-accent-t2">{hint}</p>}
      </CardContent>
    </Card>
  );
}

function RailChip({ rail, selected, onSelect }: { rail: RevenueRail; selected: boolean; onSelect: () => void }) {
  const t = useT();
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      className={`rounded-lg border px-3 py-2 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring ${
        selected ? "border-primary bg-primary/10" : "border-border hover:bg-foreground/5"
      }`}
    >
      <span className="block text-xs font-medium">{railLabel(rail.network, t)}</span>
      <span className="block text-xs tabular-nums text-muted-foreground">{t("admin.revenue.instantSuffix", { amount: formatUsd(rail.instant) })}</span>
    </button>
  );
}

function PayoutRow({ payout, busy, onCancel }: { payout: RevenuePayout; busy: boolean; onCancel: () => void }) {
  const t = useT();
  const open = OPEN_STATES.has(payout.state);
  return (
    <tr>
      <td className="px-5 py-3">
        <p className="uppercase text-xs text-muted-foreground">{payout.network}</p>
        <p className="font-mono-tech text-xs" title={payout.address}>
          {shortAddr(payout.address)}
        </p>
      </td>
      <td className="px-5 py-3 tabular-nums">{formatUsd(payout.amount)}</td>
      <td className="px-5 py-3">
        <span className={stateTone(payout.state)}>{stateLabel(payout.state, t)}</span>
      </td>
      <td className="px-5 py-3 font-mono-tech text-xs text-muted-foreground" title={payout.tx_ref || undefined}>
        {payout.tx_ref ? shortAddr(payout.tx_ref) : "—"}
      </td>
      <td className="px-5 py-3">
        <div className="flex justify-end">
          {payout.state === "queued" ? (
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onCancel}>
              {busy ? <Loader2 className="size-4 animate-spin" /> : null}
              {t("ui.cancel")}
            </Button>
          ) : (
            <span className="text-xs text-muted-foreground">{open ? t("admin.revenue.inFlight") : "—"}</span>

          )}
        </div>
      </td>
    </tr>
  );
}

function stateTone(state: string): string {
  switch (state) {
    case "completed":
      return "text-main-accent-t2";
    case "queued":
    case "processing":
      return "text-main-accent-t3";
    case "failed":
      return "text-destructive";
    default:
      return "text-muted-foreground";
  }
}

function shortAddr(value: string): string {
  return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-6)}` : value;
}
