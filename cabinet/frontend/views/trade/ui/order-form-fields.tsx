"use client";

// The form's controls and its summary rows. Stateless: the pane owns the draft, this
// draws it and reports edits back.

import { useLocale, useT } from "@evinvest/i18n/react";
import { Input, Label, OrderForm, OrderFormRow, OrderFormSubmit, Select, SelectContent, SelectItem, SelectTrigger, Spinner, ToggleGroup, ToggleGroupItem } from "@evinvest/uikit";

import { ORDER_TIFS } from "@/entities/book/lib/vocabulary";
import type { Position } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { asDecimal, orderDraftProblem, orderNotional, takerFee, type OrderContext, type OrderDraft } from "@/views/trade/lib/order-form";
import { formatUnits, formatUsdt, isZero } from "@/views/trade/lib/format";

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
      <ToggleGroup type="single" variant="outline" size="sm" value={draft.kind} onValueChange={(v) => (v === "limit" || v === "market") && onChange((d) => ({ ...d, kind: v }))} className="w-full" aria-label={t("trade.form.kind")}>
        <ToggleGroupItem value="limit" className="flex-1">
          {t("trade.form.limit")}
        </ToggleGroupItem>
        <ToggleGroupItem value="market" className="flex-1">
          {t("trade.form.market")}
        </ToggleGroupItem>
      </ToggleGroup>

      {!market && (
        <div className="space-y-1">
          <Label htmlFor="order-price" className="text-xs text-ink-soft">
            {t("trade.form.price")}
          </Label>
          <Input id="order-price" inputMode="decimal" placeholder="0.00" value={draft.price} disabled={disabled} onChange={(e) => onChange((d) => ({ ...d, price: e.target.value }))} className="w-full font-mono-tech tabular-nums" />
          {priceProblem && draft.price.trim() !== "" && <p className="text-xs text-accent-error">{t(`trade.form.problem.${problem}`, { tick: formatUsdt(context.policy?.price_tick, locale) })}</p>}
        </div>
      )}

      <div className="space-y-1">
        <div className="flex items-center justify-between">
          <Label htmlFor="order-size" className="text-xs text-ink-soft">
            {t("trade.form.size")}
          </Label>
          {!buying && position?.units && !isZero(position.units) && (
            <button type="button" className="text-xs text-primary-ink hover:underline" disabled={disabled} onClick={() => onChange((d) => ({ ...d, size: position.units ?? "" }))}>
              {t("ui.max")}
            </button>
          )}
        </div>
        <Input id="order-size" inputMode="decimal" placeholder="0" value={draft.size} disabled={disabled} onChange={(e) => onChange((d) => ({ ...d, size: e.target.value }))} className="w-full font-mono-tech tabular-nums" />
        {sizeProblem && draft.size.trim() !== "" && <p className="text-xs text-accent-error">{t(`trade.form.problem.${problem}`, { lot: formatUnits(context.policy?.lot_size, locale) })}</p>}
        {problem === "noQuote" && <p className="text-xs text-accent-error">{t("trade.form.problem.noQuote")}</p>}
      </div>

      {!market && (
        <div className="space-y-1">
          <Label className="text-xs text-ink-soft">{t("trade.form.tif")}</Label>
          <Select value={draft.tif} onValueChange={(v) => onChange((d) => ({ ...d, tif: ORDER_TIFS.find((x) => x === v) ?? d.tif }))}>
            <SelectTrigger className="w-full text-xs" disabled={disabled}>
              <span className="truncate">{t(`trade.form.tifLabel.${draft.tif}`)}</span>
            </SelectTrigger>
            <SelectContent>
              {ORDER_TIFS.map((tif) => (
                <SelectItem key={tif} value={tif}>
                  {t(`trade.form.tifLabel.${tif}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      <div className="space-y-1 border-t border-border pt-2">
        <OrderFormRow label={t(market ? "trade.form.notionalEstimate" : "trade.form.notional")} value={notional === null ? "—" : `${formatUsdt(asDecimal(notional), locale)} USDT`} />
        <OrderFormRow label={t("trade.form.fee")} value={fee === null ? "—" : `${formatUsdt(asDecimal(fee), locale)} USDT`} />
        <OrderFormRow
          label={t("trade.form.available")}
          className={cn(problem === "funds" && "text-accent-error")}
          value={buying ? (context.availableCash === null ? "—" : `${formatUsdt(context.availableCash, locale)} USDT`) : context.availableUnits === null ? "—" : t("dash.unitsAmount", { n: Number(context.availableUnits), units: formatUnits(context.availableUnits, locale) })}
        />
        {!buying && position?.units_in_orders && !isZero(position.units_in_orders) && <OrderFormRow label={t("trade.form.inOrders")} value={formatUnits(position.units_in_orders, locale)} />}
      </div>

      <OrderFormSubmit side={draft.side} disabled={disabled || problem !== null}>
        {busy ? <Spinner aria-hidden /> : t(buying ? "trade.form.submitBuy" : "trade.form.submitSell")}
      </OrderFormSubmit>
    </OrderForm>
  );
}
