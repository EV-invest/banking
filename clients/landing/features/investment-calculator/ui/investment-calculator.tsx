"use client";

import { useState } from "react";
import {
  Slider,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Button,
} from "@evinvest/uikit";
import { Heading, Text } from "@/shared/ui/text";
import { cn, notifyPlaceholder } from "@/shared/lib/utils";
import {
  calculateRoi,
  AMOUNT,
  TERMS,
  ASSET_TYPES,
  type RoiInputs,
} from "../model/roi";

// Whole-dollar USD, grouped: "$100,000".
const usd = (n: number) =>
  `$${n.toLocaleString("en-US", { maximumFractionDigits: 0 })}`;

// Interactive yield/term ROI calculator (the "#calculator" anchor). Standalone
// feature: a visitor can project returns across asset classes without it being
// entangled in the portfolio section's markup.
export function InvestmentCalculator({ className }: { className?: string }) {
  const [inputs, setInputs] = useState<RoiInputs>({
    amount: 100_000,
    term: 5,
    type: "residential",
  });
  const calculated = calculateRoi(inputs);

  return (
    <div
      id="calculator"
      className={cn(
        "border border-main-mist/10 bg-main-card p-8 grid grid-cols-1 md:grid-cols-2 gap-8",
        className
      )}
    >
      <div className="flex flex-col justify-between">
        <div>
          <span className="text-xs font-mono-tech text-main-accent-t1 tracking-widest uppercase block mb-3">
            Yield Terminal
          </span>
          <Heading scale="main">Investment Calculator</Heading>
          <Text className="mb-6">
            Project your returns across different asset classes in Quy Nhon based
            on our current fund advisory models.
          </Text>
        </div>

        <div className="space-y-4 font-mono-tech text-xs">
          <div>
            <Text asChild variant="secondary">
              <label className="uppercase block mb-3">
                Principal Investment ($ USD)
              </label>
            </Text>
            <Slider
              min={AMOUNT.min}
              max={AMOUNT.max}
              step={AMOUNT.step}
              value={inputs.amount}
              onValueChange={amount => setInputs({ ...inputs, amount })}
              aria-label="Principal investment"
              className="[&_[data-slot=slider-track]]:bg-main-black/50 [&_[data-slot=slider-range]]:bg-main-accent-t1 [&_[data-slot=slider-thumb]]:border-main-accent-t1"
            />
            <div className="flex justify-between text-main-accent-t1 font-bold mt-2">
              <span>$50k</span>
              <span className="text-sm">{usd(inputs.amount)}</span>
              <span>$1M</span>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <Text asChild variant="secondary">
                <label className="uppercase block mb-2">Term (Years)</label>
              </Text>
              <Select
                value={String(inputs.term)}
                onValueChange={value =>
                  setInputs({ ...inputs, term: Number(value) })
                }
              >
                <SelectTrigger className="w-full bg-main-black/60 border-main-mist/20 font-mono-tech">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {TERMS.map(y => (
                    <SelectItem key={y} value={String(y)}>
                      {y} Years
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div>
              <Text asChild variant="secondary">
                <label className="uppercase block mb-2">Asset Type</label>
              </Text>
              <Select
                value={inputs.type}
                onValueChange={value =>
                  setInputs({ ...inputs, type: value as RoiInputs["type"] })
                }
              >
                <SelectTrigger className="w-full bg-main-black/60 border-main-mist/20 font-mono-tech">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {ASSET_TYPES.map(t => (
                    <SelectItem key={t.value} value={t.value}>
                      {t.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
        </div>
      </div>

      {/* Output display */}
      <div className="bg-main-black/40 border border-main-mist/10 p-6 flex flex-col justify-between">
        <div className="space-y-4">
          <div>
            <Text asChild variant="secondary">
              <span className="text-[10px] font-mono-tech uppercase block mb-1">
                Estimated ROI
              </span>
            </Text>
            <span className="text-4xl font-serif-display text-main-accent-t3 font-bold">
              {calculated.roi.toLocaleString("en-US", {
                maximumFractionDigits: 1,
              })}
              %
            </span>
          </div>
          <div className="grid grid-cols-2 gap-4 border-t border-main-mist/10 pt-4">
            <div>
              <Text asChild variant="secondary">
                <span className="text-[9px] font-mono-tech uppercase block mb-0.5">
                  Total Payout
                </span>
              </Text>
              <span className="text-sm font-mono-tech text-white font-bold">
                {usd(calculated.total)}
              </span>
            </div>
            <div>
              <Text asChild variant="secondary">
                <span className="text-[9px] font-mono-tech uppercase block mb-0.5">
                  Net Profit
                </span>
              </Text>
              <span className="text-sm font-mono-tech text-main-accent-t2 font-bold">
                {usd(calculated.profit)}
              </span>
            </div>
          </div>
        </div>

        <div className="mt-6">
          <Text
            variant="secondary"
            className="text-[10px] font-light mb-4 leading-relaxed"
          >
            *Projections are based on historical performance and regional growth
            targets. Actual results may vary.
          </Text>
          <Button
            className="w-full bg-main-accent-t1 text-main-black hover:bg-main-mist hover:text-main-brand rounded-none font-mono-tech text-xs tracking-wider uppercase py-5"
            onClick={() => notifyPlaceholder("Request Advisory Session")}
          >
            Request advisory
          </Button>
        </div>
      </div>
    </div>
  );
}
