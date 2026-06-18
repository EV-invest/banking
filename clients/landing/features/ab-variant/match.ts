import type { ReactNode } from "react";
import type { ExperimentKey, Variant } from "@/shared/config/experiments";

/**
 * Selects a React node by variant value.
 *
 * TypeScript enforces exhaustiveness: every variant declared in
 * `shared/config/experiments.ts` for the given key must appear as a branch.
 * Adding a new variant to the config without updating a `match` call is a
 * compile-time error.
 *
 * @param variant  - The active variant returned by {@link getVariant}.
 * @param branches - Map from every valid variant to a React node.
 * @returns The node corresponding to the active variant.
 *
 * @example
 * ```tsx
 * const variant = await getVariant("hero");
 * return match(variant, { a: <HeroA />, b: <HeroB /> });
 * ```
 */
export function match<K extends ExperimentKey>(
  variant: Variant<K>,
  branches: { [V in Variant<K>]: ReactNode },
): ReactNode {
  return branches[variant];
}
