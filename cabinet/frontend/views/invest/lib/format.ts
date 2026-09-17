// Display helpers for fund-shares amounts. NAV/value/cash are decimal USDT strings and
// units (shares) are a separate dimension; both are formatted — and the exact math done —
// by the cabinet's one money module (`@/shared/lib/money`).

export {
  compactUnits,
  formatExactUsdt,
  formatNav,
  formatSignedUsdt,
  formatUnits,
  formatUsdt,
  fractionOfCap,
  fromBaseUnits,
  isNegative,
  isZero,
  shareBps,
  subUsdt,
  toBaseUnits,
  valence,
  valenceClass,
} from "@/shared/lib/money";
