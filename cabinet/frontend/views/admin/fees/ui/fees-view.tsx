"use client";

// Admin console — what each fund charges, and collecting it.
//
// This screen is the reason the fee plane does anything at all. The sweeper has been
// running hourly since the plane landed, but a fund charges nothing until it has a policy
// row, and until now there was no surface that could write one. So every fund in
// production charges zero — not by choice, but because the switch was unreachable.
//
// Two things happen here and they are deliberately kept apart. The LEFT column prices a
// product: terms an investor reads before they subscribe, and changing them changes what
// people pay. The RIGHT column collects what those terms have already earned: units the
// sweeper clawed back, converted to cash in one bulk settlement per period. Pricing is a
// decision; collecting is bookkeeping.
//
// What this screen does NOT do is pay the money out. Once settled, fee cash lands in the
// `fee` claim, which is exactly what `Fund revenue` withdraws on-chain — same account, an
// existing pipeline with its own rail liquidity and dispatch gates. Duplicating a payout
// form here would give an operator two doors to the same money.

import { Loader2 } from "lucide-react";
import { useMemo, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, Empty, EmptyDescription, EmptyTitle, Input, Skeleton } from "@evinvest/uikit";

import { setFeePolicy, settleFeeShares } from "@/entities/admin/api/admin-client";
import { adminAllocationsResource, feeAssessmentsResource, feePoliciesResource, feeSharesResource } from "@/entities/admin/model/admin-resource";
import type { FeePolicy } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { ago, formatUnits, formatUsd } from "@/views/admin/lib/format";
import { StaggerItem } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

// `value` is the wire enum the money plane stores; `labelKey` is only what a reader sees.
// Both lists are module scope, where no hook can run, so they carry the key and the
// `Choice` chips below resolve it against the reader's locale.
const BASES = [
  { value: "invested_capital", labelKey: "admin.fees.basis.investedCapital" },
  { value: "market_value", labelKey: "admin.fees.basis.marketValue" },
] as const;

const PERIODS = [
  { value: "monthly", labelKey: "admin.fees.period.monthly" },
  { value: "quarterly", labelKey: "admin.fees.period.quarterly" },
  { value: "semi_annual", labelKey: "admin.fees.period.semiAnnual" },
  { value: "annual", labelKey: "admin.fees.period.annual" },
] as const;

/** The house default, and what an unconfigured fund's form opens on: 2 and 20, no hurdle,
 *  charged on invested capital so the fee cannot swell with a mark the operator posted. */
const DEFAULTS = { management_bps: 200, performance_bps: 2000, hurdle_bps: 0, basis: "invested_capital", crystallization: "annual" };

export function FeesView() {
  const t = useT();
  const catalog = useResource(adminAllocationsResource);
  const policies = useResource(feePoliciesResource);
  const [service, setService] = useState("");

  const funds = catalog.data?.allocations ?? [];
  // The first fund is the default so the screen is useful without a click.
  const selected = service || funds[0]?.service || "";
  const byService = useMemo(() => new Map((policies.data?.policies ?? []).map((p) => [p.service, p])), [policies.data]);
  const policy = byService.get(selected) ?? null;

  const loading = catalog.isLoading || policies.isLoading;
  const error = catalog.data || !catalog.error ? null : errorMessage(catalog.error, t);

  return (
    <AdminScreen className="space-y-6">
      <AdminHeader eyebrow={t("nav.fees")} title={t("admin.fees.title")} subtitle={t("admin.fees.subtitle")} />

      {error && <ResourceError variant="alert" message={error} />}

      {loading ? (
        <StaggerItem>
          <Skeleton className="h-64 w-full" />
        </StaggerItem>
      ) : funds.length === 0 ? (
        <StaggerItem as={Empty} className="border">
          <EmptyTitle>{t("admin.fees.noFunds")}</EmptyTitle>
          <EmptyDescription>{t("admin.fees.noFundsHint")}</EmptyDescription>
        </StaggerItem>
      ) : (
        <>
          <FundPicker funds={funds.map((f) => ({ service: f.service, title: f.title }))} selected={selected} onSelect={setService} policies={byService} />
          <StaggerItem className="grid gap-5 lg:grid-cols-2">
            <PolicyCard key={selected} service={selected} policy={policy} />
            <CollectCard service={selected} />
          </StaggerItem>
          <StaggerItem>
            <AssessmentsCard service={selected} />
          </StaggerItem>
        </>
      )}
    </AdminScreen>
  );
}

/** One row per fund, with its rate inline so the operator can see at a glance which
 *  products are priced and which are still charging nothing. */
function FundPicker({
  funds,
  selected,
  onSelect,
  policies,
}: {
  funds: { service: string; title: string }[];
  selected: string;
  onSelect: (service: string) => void;
  policies: Map<string, FeePolicy>;
}) {
  const t = useT();
  return (
    <StaggerItem className="flex flex-wrap gap-2">
      {funds.map((fund) => {
        const policy = policies.get(fund.service);
        const active = fund.service === selected;
        return (
          <button
            key={fund.service}
            type="button"
            onClick={() => onSelect(fund.service)}
            aria-pressed={active}
            className={`rounded-lg border px-3 py-2 text-left text-sm transition-colors ${
              active ? "border-main-accent-t1 bg-main-accent-t1/10" : "border-border hover:bg-muted/50"
            }`}
          >
            <span className="block font-medium">{fund.title}</span>
            <span className="block text-xs text-muted-foreground">
              {policy?.configured ? `${pct(policy.management_bps)} / ${pct(policy.performance_bps)}` : t("admin.fees.noFee")}
            </span>
          </button>
        );
      })}
    </StaggerItem>
  );
}

function PolicyCard({ service, policy }: { service: string; policy: FeePolicy | null }) {
  const t = useT();
  const initial = policy?.configured ? policy : { ...DEFAULTS };
  const [management, setManagement] = useState(String(initial.management_bps));
  const [performance, setPerformance] = useState(String(initial.performance_bps));
  const [hurdle, setHurdle] = useState(String(initial.hurdle_bps));
  const [basis, setBasis] = useState(initial.basis);
  const [crystallization, setCrystallization] = useState(initial.crystallization);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  // Basis points, so 10000 is 100%. A rate above that is always a typo — most often a
  // percentage typed where bps were meant, which would be a hundredfold overcharge.
  const invalid = useMemo(() => {
    for (const [labelKey, raw] of [
      ["admin.fees.field.management", management],
      ["admin.fees.field.performance", performance],
      ["admin.fees.field.hurdle", hurdle],
    ] as const) {
      const n = Number(raw);
      // The field name is interpolated rather than concatenated onto the front: which end
      // of the sentence it belongs at is a per-language decision.
      if (!Number.isInteger(n) || n < 0) return t("admin.fees.err.notInteger", { field: t(labelKey) });
      if (n > 10_000) return t("admin.fees.err.tooLarge", { field: t(labelKey), pct: pct(n) });
    }
    return null;
  }, [management, performance, hurdle, t]);

  async function save() {
    if (invalid) return;
    setBusy(true);
    setProblem(null);
    setSaved(false);
    try {
      await setFeePolicy({
        service,
        management_bps: Number(management),
        performance_bps: Number(performance),
        hurdle_bps: Number(hurdle),
        basis,
        crystallization,
      });
      revalidateTag(TAG.adminFees);
      setSaved(true);
    } catch (e) {
      setProblem(e instanceof Error ? errorMessage(e, t) : t("err.feePolicySave"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="h-fit">
      <CardContent className="space-y-4 py-6">
        <div className="space-y-1">
          <p className="text-sm font-semibold">{t("admin.fees.terms")}</p>
          <p className="text-xs text-muted-foreground">
            {policy?.configured ? t("admin.fees.lastChanged", { when: ago(policy.updated_at, t) }) : t("admin.fees.notConfigured")}
          </p>
        </div>

        <div className="grid gap-3 sm:grid-cols-3">
          <BpsField label={t("admin.fees.field.management")} value={management} onChange={setManagement} hint={t("admin.fees.hint.perYear")} />
          <BpsField label={t("admin.fees.field.performance")} value={performance} onChange={setPerformance} hint={t("admin.fees.hint.ofTheGain")} />
          <BpsField label={t("admin.fees.field.hurdle")} value={hurdle} onChange={setHurdle} hint={t("admin.fees.hint.zeroForNone")} />
        </div>

        <Choice label={t("admin.fees.chargedOn")} value={basis} onChange={setBasis} options={BASES} />
        <p className="text-xs text-muted-foreground">{t("admin.fees.basisNote")}</p>

        <Choice label={t("admin.fees.lockedIn")} value={crystallization} onChange={setCrystallization} options={PERIODS} />
        <p className="text-xs text-muted-foreground">{t("admin.fees.crystallizationNote")}</p>

        {invalid && <p className="text-xs text-destructive">{invalid}</p>}
        {problem && <p className="text-xs text-destructive">{problem}</p>}
        {saved && !problem && <p className="text-xs text-main-accent-t2">{t("admin.fees.saved")}</p>}

        <Button type="button" onClick={save} disabled={busy || invalid !== null}>
          {busy && <Loader2 className="size-4 animate-spin" />}
          {policy?.configured ? t("admin.fees.updateTerms") : t("admin.fees.startCharging")}
        </Button>
      </CardContent>
    </Card>
  );
}

/** Accumulated units and the one button that turns them into cash. */
function CollectCard({ service }: { service: string }) {
  const t = useT();
  const shares = useResource(feeSharesResource, service);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);

  const data = shares.data ?? null;
  const nothing = !data || Number(data.units) <= 0;

  async function settle() {
    setBusy(true);
    setProblem(null);
    setDone(null);
    try {
      // Empty units settles the whole balance — the ordinary end-of-period call, and the
      // only one worth a button. A partial settlement is a rare, deliberate act better
      // done against the API than offered as a field nobody needs.
      const settlement = await settleFeeShares({ service, units: "" });
      // The settle moves the fee units AND the revenue figure the payout screen reads.
      revalidateTag(TAG.adminFees, TAG.adminRevenue);
      setDone(t("admin.fees.settledAtNav", { cash: formatUsd(settlement.cash), nav: settlement.nav }));
    } catch (e) {
      setProblem(e instanceof Error ? errorMessage(e, t) : t("err.feeSettle"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="h-fit">
      <CardContent className="space-y-4 py-6">
        <div className="space-y-1">
          <p className="text-sm font-semibold">{t("admin.fees.collected")}</p>
          <p className="text-xs text-muted-foreground">{t("admin.fees.collectedSub")}</p>
        </div>

        {shares.isLoading ? (
          <Skeleton className="h-16 w-full" />
        ) : (
          <dl className="space-y-2.5 text-sm">
            <Row label={t("admin.fees.unitsHeld")} value={formatUnits(data?.units)} />
            <Row label={t("admin.fees.worthAtNav")} value={`${formatUsd(data?.value)} USDT`} />
          </dl>
        )}

        <p className="text-xs text-muted-foreground">{t("admin.fees.settleNote")}</p>

        {problem && <p className="text-xs text-destructive">{problem}</p>}
        {/* Two independently complete sentences, so the settlement line and the pointer to
            the payout screen stay separate keys; the screen's own name is interpolated so it
            tracks whatever the nav calls it. The emphasis on that name is the one casualty
            of keeping the sentence whole for translators. */}
        {done && !problem && <p className="text-xs text-main-accent-t2">{`${done} ${t("admin.fees.withdrawableFrom", { screen: t("nav.revenue") })}`}</p>}

        <Button type="button" variant="outline" onClick={settle} disabled={busy || nothing}>
          {busy && <Loader2 className="size-4 animate-spin" />}
          {t("admin.fees.settleAll")}
        </Button>
        {nothing && !shares.isLoading && <p className="text-xs text-muted-foreground">{t("admin.fees.nothingToSettle")}</p>}
      </CardContent>
    </Card>
  );
}

/** The audit trail: every charge this fund has made, newest first. */
function AssessmentsCard({ service }: { service: string }) {
  const t = useT();
  const list = useResource(feeAssessmentsResource, service);
  const rows = list.data?.assessments ?? [];

  return (
    <Card>
      <CardContent className="space-y-4 py-6">
        <p className="text-sm font-semibold">{t("admin.fees.charges")}</p>
        {list.isLoading ? (
          <Skeleton className="h-24 w-full" />
        ) : rows.length === 0 ? (
          <Empty className="border">
            <EmptyTitle>{t("admin.fees.noCharges")}</EmptyTitle>
            <EmptyDescription>{t("admin.fees.noChargesHint")}</EmptyDescription>
          </Empty>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                {/* i18n-max: 14 per header — the wrapper scrolls, so a long header costs a
                    sideways drag rather than a clipped column. */}
                <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                  <th className="pb-2 font-medium">{t("admin.col.when")}</th>
                  <th className="pb-2 font-medium">{t("admin.fees.col.trigger")}</th>
                  <th className="pb-2 text-right font-medium">{t("admin.fees.field.management")}</th>
                  <th className="pb-2 text-right font-medium">{t("admin.fees.field.performance")}</th>
                  <th className="pb-2 text-right font-medium">{t("admin.fees.col.unitsTaken")}</th>
                  <th className="pb-2 text-right font-medium">{t("admin.fees.col.deferred")}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((a, i) => (
                  <tr key={`${a.assessed_at}-${i}`} className="border-b border-border/50 last:border-0">
                    <td className="py-2 text-muted-foreground">{ago(a.assessed_at, t)}</td>
                    <td className="py-2 capitalize">{a.trigger}</td>
                    <td className="py-2 text-right tabular-nums">{formatUsd(a.management)}</td>
                    <td className="py-2 text-right tabular-nums">{formatUsd(a.performance)}</td>
                    <td className="py-2 text-right tabular-nums">{formatUnits(a.charged_units)}</td>
                    {/* Non-zero means the holding could not cover the charge and the rest
                        rides to the next one. Worth its own column: it is the only reason
                        a charge collects less than it assessed. */}
                    <td className={`py-2 text-right tabular-nums ${Number(a.debt_carried) > 0 ? "text-main-accent-t3" : "text-muted-foreground"}`}>
                      {formatUsd(a.debt_carried)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function BpsField({ label, value, onChange, hint }: { label: string; value: string; onChange: (v: string) => void; hint: string }) {
  return (
    <label className="space-y-1.5 text-sm">
      <span className="block font-medium">{label}</span>
      <Input inputMode="numeric" value={value} onChange={(e) => onChange(e.target.value)} />
      <span className="block text-xs text-muted-foreground">
        {pct(Number(value) || 0)} {hint}
      </span>
    </label>
  );
}

function Choice<T extends string>({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (v: T) => void;
  options: readonly { value: T; labelKey: string }[];
}) {
  const t = useT();
  return (
    <div className="space-y-1.5 text-sm">
      <span className="block font-medium">{label}</span>
      <div className="flex flex-wrap gap-2">
        {options.map((option) => (
          <button
            key={option.value}
            type="button"
            onClick={() => onChange(option.value)}
            aria-pressed={value === option.value}
            className={`rounded-md border px-2.5 py-1.5 text-xs transition-colors ${
              value === option.value ? "border-main-accent-t1 bg-main-accent-t1/10" : "border-border hover:bg-muted/50"
            }`}
          >
            {t(option.labelKey)}
          </button>

        ))}
      </div>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="font-medium tabular-nums">{value}</dd>
    </div>
  );
}

/** Basis points as a human reads them: 200 → "2%", 250 → "2.5%". */
function pct(bps: number): string {
  const value = bps / 100;
  return `${Number.isInteger(value) ? value : Number(value.toFixed(2))}%`;
}
