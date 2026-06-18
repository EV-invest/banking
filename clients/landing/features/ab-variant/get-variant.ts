import { cookies } from "next/headers";
import {
  cookieName,
  resolveVariant,
  type ExperimentKey,
  type Variant,
} from "@/shared/config/experiments";

/**
 * Reads the visitor's assigned A/B variant from the `ab_<key>` cookie set by
 * `proxy.ts`. Falls back to the first (control) variant when the cookie is
 * missing or holds an unrecognised value.
 *
 * **Server Component only** — uses `next/headers` and will throw if called on
 * the client. Reading the cookie opts the route into dynamic rendering, which is
 * the inherent cost of cookie-based A/B (see `PATTERNS.md`).
 *
 * Sections call this themselves so the **page** stays agnostic of A/B:
 * `HomeView` just renders `<Hero />`, `<Team />`, etc.
 *
 * @param key - Experiment key declared in `shared/config/experiments.ts`.
 * @returns The variant string, narrowed to the valid union for that experiment.
 *
 * @example
 * ```tsx
 * // Inside a server wrapper component:
 * const variant = await getVariant("hero");
 * ```
 */
export async function getVariant<K extends ExperimentKey>(
  key: K,
): Promise<Variant<K>> {
  const jar = await cookies();
  return resolveVariant(key, jar.get(cookieName(key))?.value);
}
