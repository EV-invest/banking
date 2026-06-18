/**
 * @module features/ab-variant
 *
 * A/B testing utilities — client-safe exports only.
 *
 * `getVariant` uses `next/headers` and is **server-only**; import it directly:
 * ```ts
 * import { getVariant } from "@/features/ab-variant/get-variant";
 * ```
 *
 * Typical section pattern:
 * ```tsx
 * // server wrapper
 * const variant = await getVariant("hero");
 * return (
 *   <ExperimentTracker experiment="hero" variant={variant}>
 *     {match(variant, { a: <HeroA />, b: <HeroB /> })}
 *   </ExperimentTracker>
 * );
 *
 * // client island inside the section
 * const track = useExperimentEvent();
 * track("cta_clicked", { cta: "explore" }, (fire) => { fire(); scroll(); });
 * ```
 */
export { DevAbPanel } from "./dev-ab-panel";
export { ExperimentTracker, useExperimentEvent } from "./experiment-tracker";
export { match } from "./match";
