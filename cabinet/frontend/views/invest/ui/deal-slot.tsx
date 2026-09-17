"use client";

import { type ReactNode, useEffect, useRef } from "react";

// The slot a dealing panel opens in. On a phone the panel mounts below the fold: the
// header's Subscribe sits at the top of the page and the panel lands under the stats and
// notes, at the height of the fixed tab bar — a tap that changed nothing the reader could
// see. So the slot brings itself into view when it mounts and hands focus to the amount,
// which is where the next tap was going anyway.
//
// The caller keys the slot on the open panel, so switching Subscribe → Redeem remounts it
// and repeats both. `nearest` scrolls only as far as it must; the bottom margin keeps the
// tab bar from covering what it scrolled to, and `preventScroll` on the focus stops the
// browser from doing its own, un-margined, scroll after ours.
export function DealSlot({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.scrollIntoView({ block: "nearest" });
    el.querySelector<HTMLInputElement>("input")?.focus({ preventScroll: true });
  }, []);
  return (
    <div ref={ref} className="scroll-mb-[var(--cabinet-bottom-nav-h,64px)] lg:scroll-mb-0">
      {children}
    </div>
  );
}
