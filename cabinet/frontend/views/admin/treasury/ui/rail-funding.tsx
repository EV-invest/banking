"use client";

import { Check, Copy } from "lucide-react";
import { useCallback, useState } from "react";

import type { Locale } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Button } from "@evinvest/uikit";

import type { RailLiquidity } from "@/shared/contracts/admin";
import { displayAddress } from "@/shared/lib/ton-address";
import { TipAnchor } from "@/shared/tips";

const GAS_SYMBOLS: Record<string, string> = {
  bep20: "BNB",
  trc20: "TRX",
  ton: "TON",
  polygon: "POL",
};

/** The rail's hot-wallet funding picture — address + on-chain USDT/gas, "—" when the
 * treasury read was unavailable (the hub degrades to empty, never fails). */
export function RailFunding({ rail }: { rail: RailLiquidity }) {
  const t = useT();
  const locale = useLocale();
  const gasSymbol = GAS_SYMBOLS[rail.network] ?? "";
  // The hub stores TON addresses raw (`workchain:hex`) — an operator can't recognise or
  // paste that into a wallet, so render the same friendly form the deposit screen shows.
  const show = (address: string) => displayAddress(rail.network, address, { testnet: rail.is_testnet });

  return (
    <div className="space-y-2 border-t border-border pt-2.5">
      {rail.treasury_address ? (
        <div className="space-y-1">
          <div className="flex items-center gap-1.5">
            <p className="text-xs text-ink-soft">{t("nav.treasury")}</p>
            <TipAnchor anchor="admin.treasury.rail.address" />
          </div>
          <CopyableAddress address={show(rail.treasury_address)} />
        </div>
      ) : (
        <p className="text-xs text-ink-soft">{t("admin.treasury.custodyUnconfigured")}</p>
      )}
      <FundingRow label={t("admin.treasury.onchainUsdt")} value={rail.onchain_usdt ? qty(rail.onchain_usdt, locale) : undefined} />
      <FundingRow label={t("admin.treasury.gas")} value={rail.onchain_gas ? `${qty(rail.onchain_gas, locale)} ${gasSymbol}`.trimEnd() : undefined} />
      {rail.gas_station_address && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            {/* The accent parenthetical is its own complete thought, so it keeps its own key
                and its own colour rather than being folded into the label. */}
            <p className="text-xs text-ink-soft">
              {t("admin.treasury.gasStation")}{" "}
              <span className="text-positive">{t("admin.treasury.gasStationHint", { symbol: gasSymbol || t("admin.treasury.gasWord") })}</span>
            </p>
            <TipAnchor anchor="admin.treasury.rail.gas-station" />
          </div>
          <CopyableAddress address={show(rail.gas_station_address)} />
          <FundingRow
            label={t("admin.treasury.gasStationBalance")}
            value={rail.gas_station_gas ? `${qty(rail.gas_station_gas, locale)} ${gasSymbol}`.trimEnd() : undefined}
          />
        </div>
      )}
    </div>
  );
}

function FundingRow({ label, value }: { label: string; value: string | undefined }) {
  return (
    <div className="flex items-center justify-between text-xs">
      <span className="text-ink-soft">{label}</span>
      <span className="tabular-nums">{value ?? "—"}</span>
    </div>
  );
}

/** Address row with full address in a code block + copy button.
 *  Follows the same pattern as deposit-view's deposit address. */
function CopyableAddress({ address, label }: { address: string; label?: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);

  const copy = useCallback(() => {
    void navigator.clipboard.writeText(address);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [address]);

  return (
    <div className="space-y-1">
      {label && <p className="text-xs text-ink-soft">{label}</p>}
      <div className="flex items-center gap-1.5">
        <code className="flex-1 min-w-0 truncate rounded border border-border bg-secondary px-2 py-1 font-mono-tech text-xs text-ink-soft" title={address}>
          {address}
        </code>
        <Button type="button" variant="outline" icon onClick={copy} aria-label={t("admin.treasury.a11y.copy", { what: label ?? t("ui.address") })}>
          {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
        </Button>
      </div>
    </div>
  );
}

/** A native-unit decimal string → grouped display; 6 dp so a thin gas balance
 * (e.g. 0.005 BNB) doesn't round to nothing.
 *
 * Grouped in the reader's locale: this is a gas quantity, not money, so it is outside
 * `shared/lib/money.ts`'s fixed-precision policies and formats on its own. A German
 * operator reads `1.234,5 BNB`. */
function qty(value: string, locale: Locale): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return value;
  return n.toLocaleString(locale, { maximumFractionDigits: 6 });
}
