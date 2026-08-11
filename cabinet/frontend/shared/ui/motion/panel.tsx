"use client";

import { type ReactNode } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { DUR, EASE } from "./tokens";

/**
 * Presence boundary for a panel that mounts and unmounts — the admin user
 * drawer, an inline detail card, anything rendered behind a `&&`.
 *
 * Without this, closing a panel removes it from the DOM in the same frame and
 * the layout snaps shut. `AnimatePresence` keeps the outgoing node alive long
 * enough for {@link Panel} to play its exit. `initial={false}` so a panel that is
 * already open on first paint (a deep link) does not animate in.
 */
export function PanelPresence({ children }: { children: ReactNode }) {
  return <AnimatePresence initial={false}>{children}</AnimatePresence>;
}

export interface PanelProps {
  /** Which edge the panel comes from. Match it to where the panel sits. */
  from?: "right" | "bottom";
  /**
   * Animate the panel's own height when its content changes size. On by default:
   * switching between two records of different heights otherwise jumps.
   */
  resize?: boolean;
  className?: string;
  children: ReactNode;
}

const OFFSET = 12;

/**
 * The panel itself. Must be a direct child of {@link PanelPresence}, and must
 * carry a stable `key` — key it on *whether* the panel is open, not on which
 * record it shows, or every switch plays a full exit + enter. Use
 * {@link PanelSwap} inside for the record-to-record change.
 */
export function Panel({
  from = "right",
  resize = true,
  className,
  children,
}: PanelProps) {
  const reduce = useReducedMotion();
  const shift = reduce ? {} : from === "right" ? { x: OFFSET } : { y: OFFSET };

  return (
    <motion.div
      layout={resize ? "size" : false}
      className={className}
      initial={{ opacity: 0, ...shift }}
      animate={{ opacity: 1, x: 0, y: 0 }}
      exit={{ opacity: 0, ...shift }}
      transition={{ duration: DUR.base, ease: EASE.out }}
    >
      {children}
    </motion.div>
  );
}

export interface PanelSwapProps {
  /** Identity of what is being shown. A change here plays the swap. */
  swapKey: string;
  children: ReactNode;
  className?: string;
}

/**
 * Content changing *inside* a panel that stays put — picking a different user
 * while the drawer is already open. The frame holds its position and the body
 * cross-fades, so the eye tracks "same panel, new record" instead of re-reading
 * the whole thing.
 *
 * `mode="popLayout"`, not `"wait"`. `wait` holds the incoming record until the
 * outgoing one has finished leaving, so there is a stretch with no content at
 * all — and with {@link Panel}'s size animation the frame collapses toward zero
 * and springs back. `popLayout` takes the leaving record out of flow instead, so
 * the two overlap, the new height is known immediately, and the panel resizes
 * once, in one direction.
 *
 * The exit is deliberately shorter than the enter: leaving should read as a
 * dismissal, arriving as a placement.
 */
export function PanelSwap({ swapKey, children, className }: PanelSwapProps) {
  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <motion.div
        key={swapKey}
        className={className}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{
          opacity: 0,
          transition: { duration: DUR.fast * 0.6, ease: EASE.out },
        }}
        transition={{ duration: DUR.fast, ease: EASE.out }}
      >
        {children}
      </motion.div>
    </AnimatePresence>
  );
}
