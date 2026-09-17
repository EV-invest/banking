import type { Translate } from "@evinvest/i18n";

import { Logo } from "@/shared/ui/logo";

// The branded left panel of the sign-in page (Figma `cabinet/login`). Locked to the
// brand palette (white on navy, the fixed teal washes), so it deliberately does not
// follow the app's ink token.
export function BrandPanel({ t }: { t: Translate }) {
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

      {/* TODO(#385): sourced figures — these two are hand-typed marketing numbers, kept
          as they were until the one place that owns them exists. */}
      <div className="relative flex gap-8">
        <BrandStat value="18.4%" label={t("auth.stat.targetIrr")} />
        <BrandStat value="$120M+" label={t("auth.stat.aum")} />
      </div>
    </aside>
  );
}

function BrandStat({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex flex-col gap-1">
      <p className="text-2xl font-semibold text-accent-warn">{value}</p>
      <p className="text-xs text-ink-soft">{label}</p>
    </div>
  );
}
