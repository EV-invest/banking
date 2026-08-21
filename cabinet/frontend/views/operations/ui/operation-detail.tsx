"use client";

import { Link } from "@/shared/ui/cabinet-link";

import { Badge, Button, Separator } from "@evinvest/uikit";

import type { Operation } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { amountTone, dayLabel, formatUnits, formatUsdt, kindMeta, networkLabel, seconds, stateLabel, stateTone, timeLabel } from "@/views/operations/lib/format";

// The body of the operation detail panel (Figma `Cabinet · Operations` →
// `popover-operation-detail`). Rendered inside a Popover on the desktop timeline and a
// Drawer on a phone, so it owns no positioning of its own — only the content.
//
// Everything here is derived from the `Operation` row the timeline already holds; the
// panel makes no second request. That constrains what it may claim: the wire carries a
// lifecycle `state` and a `created_at`, so the progress list stops at what those two can
// prove. A per-step timestamp, a "signed by vault" leg, or a confirmation count would all
// have to be invented, and an invented number on a money surface is worse than an absent
// one.
export function OperationDetail({ operation, title, onManage }: { operation: Operation; title: string; onManage?: `/${string}` | null }) {
  const meta = kindMeta(operation.kind);
  const at = seconds(operation.created_at);
  const steps = progressFor(operation);

  return (
    <div className="flex flex-col">
      <header className="flex flex-col gap-1 px-4 pb-3 pt-4">
        <div className="flex items-center gap-2">
          <Badge className={cn("font-semibold", meta.tone)}>{meta.badge}</Badge>
          <p className="min-w-0 flex-1 truncate text-base font-semibold text-foreground">{title}</p>
          <Badge className={cn("capitalize", stateTone(operation.state))}>{stateLabel(operation.state)}</Badge>
        </div>
        <p className="text-xs text-muted-foreground">{context(operation, at)}</p>
        <p className={cn("pt-1 text-2xl font-semibold tabular-nums", amountTone(meta.direction))}>{headline(operation)}</p>
        {subheadline(operation) && <p className="text-xs text-muted-foreground">{subheadline(operation)}</p>}
      </header>

      {steps.length > 0 && (
        <>
          <Separator />
          <section className="flex flex-col gap-3 px-4 py-4">
            <h3 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">Progress</h3>
            {steps.map((step) => (
              <div key={step.label} className="flex items-center gap-2.5">
                <span
                  aria-hidden
                  className={cn("size-2.5 shrink-0 rounded-full", step.state === "done" ? "bg-main-accent-t2" : step.state === "active" ? "bg-main-accent-t3" : "bg-muted-foreground/40")}
                />
                <span className="flex min-w-0 flex-col">
                  <span className={cn("truncate text-sm", step.state === "todo" ? "text-muted-foreground" : "font-medium text-foreground")}>{step.label}</span>
                  <span className="truncate text-xs text-muted-foreground">{step.meta}</span>
                </span>
              </div>
            ))}
          </section>
        </>
      )}

      <Separator />
      <section className="flex flex-col gap-2 px-4 py-4">
        <h3 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">Details</h3>
        {/* The label is fixed and the value wraps, not the other way round. A deposit
            reference is ~50 characters and a TON address 48; letting the value size the
            row pushed it straight through the panel's right edge and over the row above. */}
        {detailsFor(operation).map(([label, value]) => (
          <div key={label} className="flex items-start gap-3">
            <span className="shrink-0 text-xs text-muted-foreground">{label}</span>
            <span className="min-w-0 flex-1 break-all text-right text-xs font-medium tabular-nums text-foreground">{value}</span>
          </div>
        ))}
      </section>

      {/* Only the two kinds with something still cancellable get a footer; for everything
          else there is no action left, and a footer would be chrome for nothing. */}
      {onManage && (
        <>
          <Separator />
          <div className="flex justify-end px-2 py-2">
            <Button asChild variant="ghost" size="sm">
              <Link href={onManage}>Manage</Link>
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

interface Step {
  label: string;
  meta: string;
  state: "done" | "active" | "todo";
}

// A deposit row exists only once the watcher confirmed it, and a subscription is an
// immutable mint — neither has a lifecycle to draw, which is also why their panel is a
// third the height of a withdrawal's.
function progressFor(operation: Operation): Step[] {
  const state = operation.state ?? "";
  const at = seconds(operation.created_at);
  const requested: Step = { label: "Requested", meta: at > 0 ? `${dayLabel(at)} ${timeLabel(at)}` : "—", state: "done" };

  if (operation.kind === "withdrawal") {
    if (state === "cancelled" || state === "failed") {
      return [requested, { label: state === "cancelled" ? "Cancelled — balance returned" : "Failed — balance returned", meta: "the reservation was voided", state: "done" }];
    }
    return [
      requested,
      state === "queued"
        ? { label: "Awaiting a liquid rail", meta: "ships as soon as the rail is topped up", state: "active" }
        : { label: "Broadcast to the network", meta: "in flight", state: "done" },
      state === "completed" ? { label: "Settled on-chain", meta: "funds have left custody", state: "done" } : { label: "Settled on-chain", meta: "waiting", state: "todo" },
    ];
  }

  if (operation.kind === "redemption") {
    if (state === "cancelled" || state === "failed") {
      return [requested, { label: state === "cancelled" ? "Cancelled — units returned" : "Failed — units returned", meta: "the reserved burn was voided", state: "done" }];
    }
    return [
      requested,
      state === "queued"
        ? { label: "Awaiting fund liquidity", meta: "priced at the NAV when it settles", state: "active" }
        : { label: "Priced at the settle NAV", meta: operation.nav ? `${formatUsdt(operation.nav)} per unit` : "—", state: "done" },
      state === "completed" ? { label: "Cash paid out", meta: "credited to your balance", state: "done" } : { label: "Cash paid out", meta: "waiting", state: "todo" },
    ];
  }

  return [];
}

function context(operation: Operation, at: number): string {
  const when = at > 0 ? `${dayLabel(at).toLowerCase()} ${timeLabel(at)}` : "—";
  if (operation.kind === "deposit" || operation.kind === "withdrawal") return `${networkLabel(operation.network)} · ${when}`;
  return `Fund · ${when}`;
}

function headline(operation: Operation): string {
  // An unsettled redemption has no cash figure at all, so the units it reserved are the
  // only true headline available.
  if (!operation.amount) return `${formatUnits(operation.units)} units`;
  const { direction } = kindMeta(operation.kind);
  const sign = direction === "in" ? "+" : direction === "out" ? "−" : "";
  return `${sign}${formatUsdt(operation.amount)} USDT`;
}

function subheadline(operation: Operation): string | null {
  if (operation.kind === "withdrawal" && operation.net_amount) return `${formatUsdt(operation.net_amount)} USDT arrives after the network fee`;
  if (operation.kind === "redemption" && !operation.amount) return "Priced when the fund settles it";
  // A fee moves units between holders on the share ledger — no cash leaves the account and
  // NAV per unit does not move, so nobody else in the fund pays for it either.
  if (operation.kind === "fee") {
    return operation.state === "partly_deferred"
      ? "Taken in units — the rest is carried to the next charge"
      : "Taken in units, not from your wallet";
  }
  return null;
}

function detailsFor(operation: Operation): [string, string][] {
  const rows: [string, string][] = [];
  switch (operation.kind) {
    case "deposit":
      rows.push(["Amount credited", `${formatUsdt(operation.amount)} USDT`]);
      rows.push(["Network", networkLabel(operation.network)]);
      if (operation.tx_ref) rows.push(["Reference", operation.tx_ref]);
      break;
    case "withdrawal":
      rows.push(["Amount debited", `${formatUsdt(operation.amount)} USDT`]);
      if (operation.fee) rows.push(["Network fee", `${formatUsdt(operation.fee)} USDT`]);
      if (operation.net_amount) rows.push(["Net sent", `${formatUsdt(operation.net_amount)} USDT`]);
      rows.push(["Network", networkLabel(operation.network)]);
      if (operation.address) rows.push(["To address", operation.address]);
      rows.push(["Reference", operation.tx_ref || "not yet broadcast"]);
      break;
    case "subscription":
      rows.push(["Cash in", `${formatUsdt(operation.amount)} USDT`]);
      rows.push(["Units minted", formatUnits(operation.units)]);
      if (operation.nav) rows.push(["Price per unit", `${formatUsdt(operation.nav)} USDT`]);
      break;
    case "redemption":
      rows.push(["Units redeemed", formatUnits(operation.units)]);
      rows.push(["Price per unit", operation.nav ? `${formatUsdt(operation.nav)} USDT` : "set at settle"]);
      rows.push(["Cash out", operation.amount ? `${formatUsdt(operation.amount)} USDT` : "set at settle"]);
      break;
    case "fee":
      // The legs first, because they are what the charge WAS; the units are how it was
      // taken. Both legs are listed even at zero here (unlike the timeline row, where
      // space is short) — on a detail panel an explicit "0.00 performance" is the answer
      // to "was I charged for the gain?", not noise.
      rows.push(["Management fee", `${formatUsdt(operation.management)} USDT`]);
      rows.push(["Performance fee", `${formatUsdt(operation.performance)} USDT`]);
      rows.push(["Units taken", formatUnits(operation.units)]);
      if (operation.nav) rows.push(["Price per unit", `${formatUsdt(operation.nav)} USDT`]);
      rows.push(["Value taken", `${formatUsdt(operation.amount)} USDT`]);
      break;
    default:
      break;
  }
  return rows;
}
