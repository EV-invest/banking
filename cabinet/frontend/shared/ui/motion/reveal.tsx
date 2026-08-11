"use client";

import { motion, useReducedMotion, type HTMLMotionProps } from "motion/react";

import { DUR, EASE, RISE, STAGGER } from "./tokens";

export interface RevealProps extends Omit<HTMLMotionProps<"div">, "ref"> {
  /** Seconds before this element moves. */
  delay?: number;
}

/**
 * A single block arriving — an alert that just appeared, a receipt, a panel
 * section. Fades and rises {@link RISE}px, once, on mount.
 *
 * Unlike the landing's equivalent this does **not** trigger on scroll. The
 * cabinet's surfaces are short and dense; a reveal that waits for the viewport
 * would leave a signed-in user looking at blank cards.
 */
export function Reveal({ delay = 0, children, ...props }: RevealProps) {
  const reduce = useReducedMotion();
  return (
    <motion.div
      initial={{ opacity: 0, y: reduce ? 0 : RISE }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: DUR.base, ease: EASE.out, delay }}
      {...props}
    >
      {children}
    </motion.div>
  );
}

export interface StaggerProps extends Omit<HTMLMotionProps<"div">, "ref"> {
  /** Seconds before the first child moves. */
  delay?: number;
  /** Seconds between siblings. Default {@link STAGGER}. */
  step?: number;
}

/**
 * A list or grid whose rows arrive in sequence. Wrap each row in
 * {@link StaggerItem}; timing is the parent's, never the row's, so reordering or
 * adding rows needs no delay arithmetic.
 *
 * Keep groups short. Past ~12 rows the tail is still arriving after the reader
 * has started reading the head, which is worse than no stagger at all — for a
 * long table, reveal the table as one block instead.
 */
export function Stagger({
  delay = 0,
  step = STAGGER,
  children,
  ...props
}: StaggerProps) {
  return (
    <motion.div
      initial="hidden"
      animate="shown"
      variants={{
        hidden: {},
        shown: { transition: { staggerChildren: step, delayChildren: delay } },
      }}
      {...props}
    >
      {children}
    </motion.div>
  );
}

/** One member of a {@link Stagger}. */
export function StaggerItem({
  children,
  ...props
}: Omit<HTMLMotionProps<"div">, "ref">) {
  const reduce = useReducedMotion();
  return (
    <motion.div
      variants={{
        hidden: { opacity: 0, y: reduce ? 0 : RISE },
        shown: {
          opacity: 1,
          y: 0,
          transition: { duration: DUR.base, ease: EASE.out },
        },
      }}
      {...props}
    >
      {children}
    </motion.div>
  );
}
