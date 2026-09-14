"use client";

// What the percentages above come to.
//
// The form takes a price the way a term sheet states one — "2 and 20" — while the money
// plane stores and charges basis points. Both belong on screen: the bps figure is what
// this screen is about to write, and the money figure is the only form in which a fee is
// actually argued about. Neither is an input, so nothing here can drift from what saves.

import { useT } from "@evinvest/i18n/react";

import { pct } from "@/shared/lib/rate";
import { formatUsd } from "@/views/admin/lib/format";
import { FIELD_LABEL_KEY, RATE_FIELDS, type RateField } from "@/views/admin/fees/lib/schedule";

/** The reference position the worked example prices. A round hundred thousand: big enough
 *  that the management line lands on a figure worth arguing about, round enough that a
 *  reader can move the decimal point to their own fund size in their head. */
const REFERENCE_POSITION = 100_000;

export function Showcase({ bps }: { bps: Record<RateField, number | null> }) {
  const t = useT();

  // Withheld rather than guessed while any rate is unparsable: a sentence assembled from
  // a field the form is about to reject would price terms nobody can save.
  const example =
    bps.management === null || bps.performance === null || bps.hurdle === null
      ? null
      : t(bps.hurdle > 0 ? "admin.fees.showcase.exampleHurdle" : "admin.fees.showcase.example", {
          amount: formatUsd(REFERENCE_POSITION),
          management: formatUsd((REFERENCE_POSITION * bps.management) / 10_000),
          performance: pct(bps.performance),
          hurdle: pct(bps.hurdle),
        });

  return (
    <div className="space-y-3 rounded-lg border border-border bg-muted/30 p-3">
      <p className="text-xs font-medium text-muted-foreground">{t("admin.fees.showcase.title")}</p>
      <dl className="grid gap-3 sm:grid-cols-3">
        {RATE_FIELDS.map((field) => (
          <div key={field} className="space-y-0.5">
            <dt className="text-xs text-muted-foreground">{t(FIELD_LABEL_KEY[field])}</dt>
            <dd className="text-sm tabular-nums">
              {bps[field] === null ? (
                <span className="text-muted-foreground">—</span>
              ) : (
                <>
                  <span className="font-medium">{pct(bps[field])}</span>{" "}
                  <span className="text-xs text-muted-foreground">{t("admin.fees.showcase.bps", { n: bps[field] })}</span>
                </>
              )}
            </dd>
          </div>
        ))}
      </dl>
      {example && <p className="text-xs text-muted-foreground">{example}</p>}
    </div>
  );
}
