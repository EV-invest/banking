import type { Locale, Translate } from "@evinvest/i18n";

import { formatFundFigures } from "@/shared/lib/fund-figures";
import { Logo } from "@/shared/ui/logo";

// The branded left panel of the sign-in page (Figma `cabinet/login`). Locked to the
// brand palette (white on navy, the fixed teal washes), so it deliberately does not
// follow the app's ink token.
export function BrandPanel({ t, locale }: { t: Translate; locale: Locale }) {
  // The same two figures the landing's hero shows, from the one place that owns them
  // (`shared/config/fund-figures`) — this panel used to hand-type a different pair.
  const figures = formatFundFigures(locale);
  return (
    <aside className="relative hidden w-150 shrink-0 flex-col justify-between overflow-hidden bg-brand p-16 lg:flex">
      {/* Both washes are bespoke art direction with no equivalent on the colour scale, so
          they are declared as CSS rather than smuggled in as arbitrary Tailwind values. */}
      {/* big soft teal wash */}
      <div
        className="pointer-events-none absolute -bottom-40 left-24 size-205 rounded-full blur-3xl"
        style={{ backgroundImage: "radial-gradient(circle,rgba(72,216,196,0.6),rgba(42,157,143,0.32) 46%,transparent 74%)" }}
      />
      {/* brighter inner core */}
      <div
        className="pointer-events-none absolute bottom-20 left-64 size-105 rounded-full blur-2xl"
        style={{ backgroundImage: "radial-gradient(circle,rgba(120,240,216,0.55),transparent 60%)" }}
      />

      <div className="relative">
        <Logo className="h-10 w-auto text-ink" />
      </div>

      <div className="relative flex max-w-md flex-col gap-5">
        {/* The brand mark itself, not a phrase — it reads "EV INVEST" in every locale. */}
        <p className="text-xs font-semibold tracking-widest text-primary-ink">EV INVEST</p>
        <h2 className="text-5xl font-semibold leading-tight text-white">{t("auth.brandHeadline")}</h2>
        <p className="text-base leading-6 text-ink-soft">{t("auth.brandBlurb")}</p>
      </div>

      <div className="relative flex flex-col gap-4">
        <div className="flex gap-8">
          <BrandStat value={figures.targetIrr} label={t("auth.stat.targetIrr")} />
          <BrandStat value={figures.closingTarget} label={t("auth.stat.closingTarget")} />
        </div>
        {/* Only once the owner has dated the figures: a placeholder date beside a return
            figure would read as a fact (see `FUND_FIGURES.asOf`). */}
        {figures.asOf !== undefined && <p className="text-xs text-ink-soft">{t("auth.stat.asOf", { date: figures.asOf })}</p>}
        {/* The risk beside the benefit, at the same place on the page: a target is not a
            forecast, and the panel must not read as a promise. */}
        <p className="text-xs leading-5 text-ink-soft">{t("auth.stat.risk")}</p>
      </div>
    </aside>
  );
}

function BrandStat({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex flex-col gap-1">
      <p className="text-2xl font-semibold text-accent-warn tabular-nums">{value}</p>
      <p className="text-xs text-ink-soft">{label}</p>
    </div>
  );
}
