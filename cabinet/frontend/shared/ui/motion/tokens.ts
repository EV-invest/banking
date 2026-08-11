// Motion tokens — the design-system half of `motion`. Every animation in the
// cabinet picks its curve and duration from here, the same way colour picks from
// the `ev/*` variables: one place to retune the feel of the whole surface, and no
// per-screen easing invented on the spot.
//
// The cabinet is a money surface, so its motion is quieter than the landing's:
// shorter, smaller travel, never decorative. Motion here has one job — to say
// *what changed* — and anything that delays a number appearing is wrong.

/** Cubic-bezier control points, in the shape `motion` wants them. */
export const EASE = {
  /** Default. Fast start, long settle — every entrance. */
  out: [0.22, 1, 0.36, 1],
  /** Symmetric; for state that reverses (a panel opening and closing). */
  inOut: [0.65, 0, 0.35, 1],
} as const;

/** Seconds. `fast` is the house default here — the landing's is twice this. */
export const DUR = {
  fast: 0.18,
  base: 0.28,
  slow: 0.4,
} as const;

/** Pixels a revealing element travels. Deliberately small. */
export const RISE = 8;

/** Seconds between siblings in a staggered list. */
export const STAGGER = 0.035;
