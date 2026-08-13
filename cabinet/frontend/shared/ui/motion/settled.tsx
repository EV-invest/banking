"use client";

import { type ReactNode, useState } from "react";
import { motion, useReducedMotion } from "motion/react";

import { DUR, EASE, RISE } from "./tokens";

export interface SettledProps {
  /** True while the data this surface needs is still in flight. */
  loading: boolean;
  /** What stands in for the content meanwhile — a `Skeleton`, or a group of them. */
  skeleton: ReactNode;
  children: ReactNode;
  className?: string;
}

/**
 * The skeleton → content handover. Every data surface in the cabinet swapped its
 * skeleton for real content in a single frame, which reads as a flicker: the eye
 * registers that *something* changed without registering *what*. Here the
 * skeleton is replaced and the content fades and rises the last few pixels into
 * its place.
 *
 * Two things it deliberately does not do:
 *
 * - **No cross-fade.** Skeleton and content would have to overlap, which means
 *   taking one out of flow and animating a height — expensive, and it makes the
 *   surface jump when the real content is a different size.
 * - **No entrance when no skeleton was shown.** Data that is already present on
 *   the first render (a cached or server-supplied value) cuts straight in;
 *   fading it would invent a delay that the data never had.
 *
 * The caller still owns the empty and error states — pass them as `children`,
 * guarded on the same condition as `loading`, and they arrive with the same
 * motion as content would.
 */
export function Settled({
  loading,
  skeleton,
  children,
  className,
}: SettledProps) {
  const reduce = useReducedMotion();

  // Adjusting state during render rather than in an effect, which is what lets
  // the entrance play on the very first render after `loading` drops: an effect
  // would paint the content opaque once and only then start the fade, and that
  // flash is more noticeable than the cut it was meant to replace.
  const [wasLoading, setWasLoading] = useState(loading);
  const [sawSkeleton, setSawSkeleton] = useState(false);
  if (wasLoading !== loading) {
    setWasLoading(loading);
    if (wasLoading) setSawSkeleton(true);
  }

  // The skeleton fades in too, rather than being thrown up the instant the
  // surface mounts. A skeleton that appears in one frame is its own small jolt —
  // and it is the FIRST thing anyone sees on a cold load, so it sets the tone for
  // the handover that follows. Deliberately quicker than the content's entrance:
  // it is scaffolding announcing that work is happening, not the answer.
  //
  // `key` is what keeps this honest. Both branches render a motion.div, so
  // without distinct keys React would reconcile skeleton → content as the same
  // element, `initial` would never re-run, and the content would inherit the
  // skeleton's opacity instead of playing its own entrance.
  if (loading)
    return (
      <motion.div
        key="skeleton"
        className={className}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: DUR.fast, ease: EASE.out }}
      >
        {skeleton}
      </motion.div>
    );

  if (!sawSkeleton) return <div className={className}>{children}</div>;

  return (
    <motion.div
      key="content"
      className={className}
      initial={{ opacity: 0, y: reduce ? 0 : RISE }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: DUR.base, ease: EASE.out }}
    >
      {children}
    </motion.div>
  );
}
