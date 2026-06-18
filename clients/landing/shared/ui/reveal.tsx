"use client";

import { motion, useReducedMotion, type HTMLMotionProps } from "motion/react";

// A restrained scroll-reveal: a short fade + 16px rise, once, triggered just
// before the element is fully in view. Institutional, not flashy — and it
// collapses to a plain fade (no movement) under prefers-reduced-motion.
interface RevealProps extends HTMLMotionProps<"div"> {
  /** Stagger offset in seconds when revealing a row/grid of items. */
  delay?: number;
}

export function Reveal({ delay = 0, children, ...props }: RevealProps) {
  const reduce = useReducedMotion();
  return (
    <motion.div
      initial={{ opacity: 0, y: reduce ? 0 : 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-80px" }}
      transition={{ duration: 0.6, ease: [0.22, 1, 0.36, 1], delay }}
      {...props}
    >
      {children}
    </motion.div>
  );
}
