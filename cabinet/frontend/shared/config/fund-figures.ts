// The fund's headline figures — the ONLY place in the cabinet a number about the fund is
// typed.
//
// MIRROR of the public site's `frontend/shared/config/fund-figures.ts` (site_conductor),
// field for field — less its `kycTiming`, which is an English sentence and lives in the
// catalogue here as `kyc.dialog.timeBody`. The landing's hero and the cabinet's sign-in panel quote the same fund,
// and a visitor who clicks through from one to the other must not watch it contradict
// itself (site_conductor #204, banking #385). The two repos share no code yet; until this
// object moves into an `@evinvest/*` package, a change on either side is a change on both.
//
// TODO(#385): owner to confirm figures and as-of date. The values below are what the site
// already showed before they were centralised, not sourced facts; `asOf` stays `undefined`
// — and the "as of" line stays off the page — until the owner names the date the figures
// were confirmed on.

export interface FundFigures {
  /** Target IRR, percent per annum. Rendered as a floor: "16.4% +". */
  readonly targetIrrPct: number;
  /** Hard cap the fund closes at, in USD millions. Rendered "$100M". */
  readonly closingTargetUsdM: number;
  /**
   * ISO 8601 calendar date the figures were last confirmed on. `undefined` means nobody
   * has confirmed them yet and the "as of" line MUST NOT render — a placeholder date next
   * to a return figure reads as a fact.
   */
  readonly asOf: string | undefined;
  /**
   * Minimum subscription, USD. `undefined` means the owner has not set one and copy MUST
   * omit the sentence — never render a placeholder amount.
   */
  readonly minSubscriptionUsd: number | undefined;
}

export const FUND_FIGURES: FundFigures = {
  targetIrrPct: 16.4,
  closingTargetUsdM: 100,
  asOf: undefined,
  minSubscriptionUsd: undefined,
};
