"use client";

import { useEffect, useState } from "react";

// Below `lg` the operation detail is a bottom sheet rather than a popover — a 380px panel
// anchored to a row does not fit a 390px viewport. The breakpoint is read at runtime
// because the two presentations are different components, not one component with
// different CSS: a media query alone cannot swap a Popover for a Drawer.
//
// Starts `false` and corrects in an effect. That is safe here because the surface's first
// paint is a loading skeleton (the timeline is fetched in an effect too), so no row — and
// therefore no trigger — exists until after this has settled.
const COMPACT = "(max-width: 1023px)";

export function useIsCompact(): boolean {
  const [compact, setCompact] = useState(false);

  useEffect(() => {
    const query = window.matchMedia(COMPACT);
    const sync = () => setCompact(query.matches);
    sync();
    query.addEventListener("change", sync);
    return () => query.removeEventListener("change", sync);
  }, []);

  return compact;
}
