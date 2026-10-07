"use client";

// The form's controls and its summary rows. Stateless: the pane owns the draft, this
// draws it and reports edits back.

import type { Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Input, Label, OrderForm, OrderFormRow, OrderFormSubmit, Select, SelectContent, SelectItem, SelectTrigger, Spinner, ToggleGroup, ToggleGroupItem } from "@evinvest/uikit";

import { ORDER_TIFS } from "@/entities/book/lib/vocabulary";
import type { Position } from "@/shared/contracts";
import type { OrderTif } from "@/shared/contracts/book";
import { cn } from "@/shared/lib/cn";
import { asDecimal, orderDraftProblem, orderNotional, takerFee, type OrderContext, type OrderDraft } from "@/views/trade/lib/order-form";
import { formatUnits, formatUsdt, isZero } from "@/views/trade/lib/format";

const tifLabels = (t: Translate): Record<OrderTif, string> => ({
  gtc: t("trade.form.tifLabel.gtc", "Good till cancelled"),
  ioc: t("trade.form.tifLabel.ioc", "Immediate or cancel"),
  alo: t("trade.form.tifLabel.alo", "Post-only"),
});

const tifHints = (t: Translate): Record<OrderTif, string> => ({
  gtc: t("trade.form.tifHint.gtc", "Rests on the book until it fills or you cancel it."),
  ioc: t("trade.form.tifHint.ioc", "Fills whatever it can right now; the rest is cancelled at once."),
  alo: t("trade.form.tifHint.alo", "Only rests on the book: it is refused rather than filled against an existing order."),
});

export function OrderFormFields({
  draft,
  context,
  position,
  disabled,
  busy,
  onChange,
  onSubmit,
}: {
  draft: OrderDraft;
  context: OrderContext;
  position: Position | null;
  disabled: boolean;
  busy: boolean;
  onChange: (update: (draft: OrderDraft) => OrderDraft) => void;
  onSubmit: () => void;
}) {
  const t = useT();
  const locale = useLocale();
  const problem = orderDraftProblem(draft, context);
  const notional = orderNotional(draft, context);
  const fee = notional === null ? null : takerFee(notional, draft, context.policy);
  const buying = draft.side === "buy";
  const market = draft.kind === "market";
  // Which figure the problem points at — so the hint sits under the field it is about.
  const priceProblem = problem === "price" || problem === "tick";
  const sizeProblem = problem === "size" || problem === "lot" || problem === "funds";

  return (
    <OrderForm onSubmit={onSubmit}>
      <ToggleGroup type="single" variant="outline" size="sm" value={draft.kind} onValueChange={(v) => (v === "limit" || v === "market") && onChange((d) => ({ ...d, kind: v }))} className="w-full" aria-label={t("trade.form.kind", "Order type")}>
        <ToggleGroupItem value="limit" className="flex-1">
          {t("trade.form.limit", "Limit")}
        </ToggleGroupItem>
        <ToggleGroupItem value="market" className="flex-1">
          {t("trade.form.market", "Market")}
        </ToggleGroupItem>
      </ToggleGroup>

      {!market && (
        <div className="space-y-1">
          <Label htmlFor="order-price" className="text-xs text-ink-soft">
            {t("trade.form.price", "Price (USDT)")}
          </Label>
          <Input id="order-price" inputMode="decimal" placeholder="0.00" value={draft.price} disabled={disabled} onChange={(e) => onChange((d) => ({ ...d, price: e.target.value }))} className="w-full font-mono-tech tabular-nums" />
          {priceProblem && draft.price.trim() !== "" && <p className="text-xs text-accent-error">{problem === "tick" ? t("trade.form.problem.tick", "Price must be a multiple of {tick}", { tick: formatUsdt(context.policy?.price_tick, locale) }) : t("trade.form.problem.price", "Enter a price above zero")}</p>}
        </div>
      )}

      <div className="space-y-1">
        <div className="flex items-center justify-between">
          <Label htmlFor="order-size" className="text-xs text-ink-soft">
            {t("trade.form.size", "Size (units)")}
          </Label>
          {!buying && position?.units && !isZero(position.units) && (
            <button type="button" className="text-xs text-primary-ink hover:underline" disabled={disabled} onClick={() => onChange((d) => ({ ...d, size: position.units ?? "" }))}>
              {t("ui.max", "Max")}
            </button>
          )}
        </div>
        <Input id="order-size" inputMode="decimal" placeholder="0" value={draft.size} disabled={disabled} onChange={(e) => onChange((d) => ({ ...d, size: e.target.value }))} className="w-full font-mono-tech tabular-nums" />
        {sizeProblem && draft.size.trim() !== "" && <p className="text-xs text-accent-error">{problem === "lot" ? t("trade.form.problem.lot", "Size must be a multiple of {lot}", { lot: formatUnits(context.policy?.lot_size, locale) }) : problem === "funds" ? t("trade.form.problem.funds", "More than you have available") : t("trade.form.problem.size", "Enter a size above zero")}</p>}
        {problem === "noQuote" && <p className="text-xs text-accent-error">{t("trade.form.problem.noQuote", "Nothing on the other side to trade against — place a limit order instead.")}</p>}
      </div>

      {!market && (
        <div className="space-y-1">
          <Label className="text-xs text-ink-soft">{t("trade.form.tif", "Time in force")}</Label>
          <Select value={draft.tif} onValueChange={(v) => onChange((d) => ({ ...d, tif: ORDER_TIFS.find((x) => x === v) ?? d.tif }))}>
            <SelectTrigger className="w-full text-xs" disabled={disabled}>
              <span className="truncate">{tifLabels(t)[draft.tif]}</span>
            </SelectTrigger>
            <SelectContent>
              {ORDER_TIFS.map((tif) => (
                <SelectItem key={tif} value={tif}>
                  {tifLabels(t)[tif]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {/* One line per choice, shown for the one selected — the labels alone are exchange
              shorthand a first-time reader cannot decode. */}
          <p className="text-xs text-ink-soft">{tifHints(t)[draft.tif]}</p>
        </div>
      )}

      <div className="space-y-1 border-t border-border pt-2">
        <OrderFormRow label={(market ? t("trade.form.notionalEstimate", "Est. value") : t("trade.form.notional", "Order value"))} value={notional === null ? "—" : `${formatUsdt(asDecimal(notional), locale)} USDT`} />
        <OrderFormRow label={t("trade.form.fee", "Taker fee")} value={fee === null ? "—" : `${formatUsdt(asDecimal(fee), locale)} USDT`} />
        <OrderFormRow
          label={t("trade.form.available", "Available")}
          className={cn(problem === "funds" && "text-accent-error")}
          value={buying ? (context.availableCash === null ? "—" : `${formatUsdt(context.availableCash, locale)} USDT`) : context.availableUnits === null ? "—" : t("dash.unitsAmount", "{n, plural, one {{units} unit} other {{units} units}}", { n: Number(context.availableUnits), units: formatUnits(context.availableUnits, locale) })}
        />
        {!buying && position?.units_in_orders && !isZero(position.units_in_orders) && <OrderFormRow label={t("trade.form.inOrders", "In orders")} value={formatUnits(position.units_in_orders, locale)} />}
      </div>

      <OrderFormSubmit side={draft.side} disabled={disabled || problem !== null}>
        {busy ? <Spinner /> : (buying ? t("trade.form.submitBuy", "Place buy order") : t("trade.form.submitSell", "Place sell order"))}
      </OrderFormSubmit>
    </OrderForm>
  );
}
