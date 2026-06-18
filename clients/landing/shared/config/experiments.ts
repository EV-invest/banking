// Single source of truth for A/B experiments. `proxy.ts` reads this to assign a
// `ab_<key>` cookie per visitor; the server reads the cookie to pick a variant;
// the dev panel reads it to enumerate togglable variants. Keep it plain data so
// it can be imported from the proxy (nodejs runtime), Server Components, and
// Client Components alike.
export const experiments = {
  hero: { variants: ["a", "b"], weights: [0.5, 0.5] },
  team: { variants: ["a", "b"], weights: [0.5, 0.5] },
} as const;

export type ExperimentKey = keyof typeof experiments;
export type Variant<K extends ExperimentKey> =
  (typeof experiments)[K]["variants"][number];

// Cookie name carrying the assigned variant for an experiment.
export function cookieName(key: ExperimentKey): string {
  return `ab_${key}`;
}

// Weighted per-device pick (Math.random, no user-id hashing). Weights need not
// sum to 1 — they're normalized by their total.
export function pickVariant<K extends ExperimentKey>(key: K): Variant<K> {
  const { variants, weights } = experiments[key];
  const total = weights.reduce((sum, w) => sum + w, 0);
  let r = Math.random() * total;
  for (let i = 0; i < variants.length; i++) {
    r -= weights[i];
    if (r < 0) return variants[i] as Variant<K>;
  }
  return variants[variants.length - 1] as Variant<K>;
}

// Coerce a raw cookie value to a valid variant, falling back to the first
// (control) variant when the cookie is missing or unrecognized.
export function resolveVariant<K extends ExperimentKey>(
  key: K,
  raw: string | undefined,
): Variant<K> {
  const { variants } = experiments[key];
  return (variants as readonly string[]).includes(raw ?? "")
    ? (raw as Variant<K>)
    : (variants[0] as Variant<K>);
}
