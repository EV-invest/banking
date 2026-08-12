"use client";

import { useLayoutEffect, useRef } from "react";
import { animate, useReducedMotion } from "motion/react";

import { DUR, EASE } from "./tokens";

export interface AnimatedNumberProps {
  /** The figure to display. Animates from whatever is on screen to this. */
  value: number;
  /**
   * How to render the running figure. Must be referentially stable — a module
   * function, or something memoised — because a new identity restarts the
   * animation. Every money formatter in `shared/lib/money` qualifies.
   */
  format: (n: number) => string;
  className?: string;
}

/**
 * A figure that travels to its new value instead of being replaced by it.
 *
 * On a balance this is the difference between "the number is 12,480" and "the
 * number *became* 12,480" — after a deposit settles or a poll lands, the motion
 * is the only thing that says which digits changed and in which direction.
 *
 * Two deliberate implementation choices:
 *
 * - **The DOM is written directly, not through React.** `animate()` drives a
 *   plain number and `onUpdate` sets `textContent`. A 60fps count that went
 *   through `setState` would re-render this component ~40 times per second and,
 *   through it, everything the parent re-renders with it. Nothing here needs to
 *   be in the React tree, so it isn't.
 * - **`useLayoutEffect`, not `useEffect`.** The JSX renders the *final* string
 *   so that SSR, a crawler, or a JS failure all show the true figure. The layout
 *   effect overwrites it with the starting figure before the browser paints, so
 *   the correct-but-not-yet-animated value is never visible. In a plain effect
 *   that overwrite lands after a paint and the number visibly snaps back to the
 *   start before counting up.
 *
 * Pair with `tabular-nums` at the call site. Proportional digits change width as
 * they cycle, and a figure that jitters sideways while it counts is worse than
 * one that simply appears.
 */
export function AnimatedNumber({
  value,
  format,
  className,
}: AnimatedNumberProps) {
  const reduce = useReducedMotion();
  const ref = useRef<HTMLSpanElement>(null);
  // What is currently on screen. Starts at 0 so the first appearance counts up
  // from nothing; afterwards it is wherever the last animation finished, so a
  // refresh travels the actual delta rather than restarting from zero.
  const shown = useRef(0);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (reduce || shown.current === value) {
      shown.current = value;
      el.textContent = format(value);
      return;
    }
    const from = shown.current;
    el.textContent = format(from);
    const controls = animate(from, value, {
      duration: DUR.slow,
      ease: EASE.out,
      onUpdate: (v) => {
        shown.current = v;
        el.textContent = format(v);
      },
    });
    return () => {
      controls.stop();
      // Leave `shown` wherever it stopped: an interrupted count that is
      // restarted should continue from what the eye last saw, not jump.
    };
  }, [value, format, reduce]);

  return (
    <span ref={ref} className={className}>
      {format(value)}
    </span>
  );
}
