import { Logo as BrandLogo } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";

// The EV Investment mark, owned by `@evinvest/uikit` — the same artwork the landing
// header masks, so the two surfaces cannot drift apart again. This file used to carry
// its own Figma export (node 464:208) of the same lockup, which had dropped the
// artwork's stroke pass and re-canvassed it at 47×40: the cabinet's mark rendered ~6%
// lighter than the landing's and sat off-centre in its box.
//
// Monochrome — paints with the current text color, so callers tint it via `text-*`
// (mist on dark surfaces). uikit renders a CSS-masked <span>, which has no intrinsic
// size, so the aspect ratio is pinned here and callers keep sizing by height alone.
export function Logo({ className }: { className?: string }) {
  return <BrandLogo className={cn("aspect-(--ev-logo-aspect)", className)} />;
}
