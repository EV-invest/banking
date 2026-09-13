// The access vocabulary, rendered. Mirrors `contracts::allocation::access` — `hidden <
// view < invest`, and a grant may only carry the top two (a grant that lowered a holder
// below the default would make "revoke" ambiguous).

import type { Translate } from "@evinvest/i18n";

import type { AllocationAccessLevel, AllocationGrantLevel } from "@/shared/contracts/admin";

/** Every level, lowest first — what the row's default-access picker offers. */
export const ACCESS_LEVELS: readonly AllocationAccessLevel[] = ["hidden", "view", "invest"];

/** The levels a grant may carry — what the grant form's level picker offers. */
export const GRANT_LEVELS: readonly AllocationGrantLevel[] = ["view", "invest"];

const ACCESS_LABEL_KEYS: Record<AllocationAccessLevel, string> = {
  hidden: "admin.alloc.access.hidden",
  view: "admin.alloc.access.view",
  invest: "admin.alloc.access.invest",
};

export function accessLabel(level: AllocationAccessLevel, t: Translate): string {
  return t(ACCESS_LABEL_KEYS[level]);
}

/** Tailwind token classes for an access-level chip — the same three tiers the row's state
 *  chip already uses (`STATE_TONE` in `allocations-view.tsx`), read here for a second axis. */
export function accessTone(level: AllocationAccessLevel): string {
  switch (level) {
    case "invest":
      return "border-main-accent-t2/40 bg-main-accent-t2/10 text-main-accent-t2";
    case "view":
      return "border-main-accent-t3/40 bg-main-accent-t3/10 text-main-accent-t3";
    case "hidden":
      return "border-border text-muted-foreground";
  }
}
