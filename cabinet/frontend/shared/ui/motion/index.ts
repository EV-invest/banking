// Public API of the cabinet's motion slice. Import from `@/shared/ui/motion` —
// never from a file inside it, and never from `motion/react` directly in a view:
// the point of the slice is that curves, durations and travel distances are
// decided once, in ./tokens, rather than per screen.
//
// Four primitives cover this surface:
//   Settled   — a skeleton hands over to the content it stood in for
//   AnimatedNumber — a figure travels to its new value instead of being replaced
//   Panel     — a popup/drawer mounts, unmounts, and swaps what it shows
//   Reveal    — one block arrives on mount
//   Stagger   — a screen, list or grid arrives in sequence (+ StaggerItem per part)
//
// All of them respect `prefers-reduced-motion`, animate only `opacity` and
// `transform`, and play once. The cabinet is a money surface: motion here exists
// to say what changed, never to decorate, and must never delay a figure landing
// on screen.
//
// A screen arrives through `Stagger` on the container it already has, with each
// section given its own element via `as` — never a new wrapper, because the
// sections are grid and flex items carrying their own placement (see
// ./element). Sections publish "still arriving" to everything inside them, and
// `Settled` reads it so a skeleton handover caught mid-entrance fades without
// travelling: one movement per card, never two composed (see ./entrance).
//
// The sibling landing (`site_conductor`) has its own slice with the same names
// and slower tokens. They are intentionally not shared: this one has no
// scroll-triggered variants, because a signed-in surface must not make someone
// scroll to make their balance appear.
export { Settled, type SettledProps } from "./settled";
export { AnimatedNumber, type AnimatedNumberProps } from "./number";
export {
  Panel,
  PanelPresence,
  PanelSwap,
  type PanelProps,
  type PanelSwapProps,
} from "./panel";
export {
  Reveal,
  Stagger,
  StaggerItem,
  type RevealProps,
  type StaggerProps,
  type StaggerItemProps,
} from "./reveal";
export { DUR, EASE, RISE, STAGGER, SECTION_STAGGER } from "./tokens";
