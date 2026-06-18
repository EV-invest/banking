// ROI projection model for the investment calculator feature. Pure + UI-free so
// it can be unit-tested and reused; the component below only renders it.

export type AssetType = "residential" | "commercial";

export interface RoiInputs {
  amount: number;
  term: number;
  type: AssetType;
}

export interface RoiResult {
  /** Total payout ($). */
  total: number;
  /** Net profit ($). */
  profit: number;
  /** ROI percentage (e.g. 338.2). */
  roi: number;
}

// Principal slider bounds ($ USD).
export const AMOUNT = { min: 50_000, max: 1_000_000, step: 10_000 } as const;

// Selectable terms (years).
export const TERMS = [3, 5, 7, 10] as const;

// Asset classes the fund advises on, with their display labels.
export const ASSET_TYPES: { value: AssetType; label: string }[] = [
  { value: "residential", label: "Luxury Villa" },
  { value: "commercial", label: "Commercial Hub" },
];

// Per-class advisory model: annual yield + annual appreciation, compounded.
const MODEL: Record<AssetType, { rate: number; appreciation: number }> = {
  residential: { rate: 0.085, appreciation: 0.15 },
  commercial: { rate: 0.12, appreciation: 0.18 },
};

export function calculateRoi({ amount, term, type }: RoiInputs): RoiResult {
  const { rate, appreciation } = MODEL[type];
  const total = amount * Math.pow(1 + (rate + appreciation), term);
  const profit = total - amount;
  return {
    total,
    profit,
    roi: (profit / amount) * 100,
  };
}
