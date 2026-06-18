import {
  experiments,
  cookieName,
  type ExperimentKey,
} from "@/shared/config/experiments";

const COOKIE_MAX_AGE = 60 * 60 * 24 * 30; // 30 days, matches proxy.ts

/**
 * Reads a cookie value by name from `document.cookie`. Client-only.
 */
export function readCookie(name: string): string | undefined {
  return document.cookie
    .split("; ")
    .find(c => c.startsWith(`${name}=`))
    ?.split("=")[1];
}

/**
 * Writes the chosen variant to the `ab_<key>` cookie (same shape `proxy.ts`
 * sets), so the next server render picks it up. Client-only; caller is expected
 * to follow with `router.refresh()` to re-render the new variant.
 */
export function writeVariant(key: ExperimentKey, value: string): void {
  document.cookie = `${cookieName(key)}=${value};path=/;max-age=${COOKIE_MAX_AGE};samesite=lax`;
}

/**
 * The variant one `step` (+1 / -1) away from `current` for an experiment,
 * wrapping around its declared variant list. Lands only on real variants, so
 * vim-style `l`/`h` cycling can never select a variant that doesn't exist.
 */
export function nextVariant(
  key: ExperimentKey,
  current: string,
  step: 1 | -1,
): string {
  const { variants } = experiments[key];
  const idx = Math.max(0, variants.indexOf(current as never));
  return variants[(idx + step + variants.length) % variants.length];
}
