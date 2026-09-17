/**
 * The first-run path — verify, deposit, invest — as three steps with a state each.
 *
 * Derived, never stored. The cabinet holds no onboarding record and needs none: each step is
 * done exactly when the fact behind it is true on the hub — a tier above 0, a balance above
 * zero, units held in any allocation. The block reads the same values the dashboard already
 * paints, so it cannot disagree with the figures beside it.
 *
 * Pure, so it can be run: the cabinet has no component tests, and `node --test` over
 * `*.test.ts` is the only level this rule can be checked at. No imports for the same reason —
 * the runner strips types but does not resolve `@/` paths.
 */
export interface ChecklistRead {
  /** The tier to believe — the identity plane's when it answered, the profile's otherwise. */
  level: number;
  /**
   * The verification attempt running right now, and whether its vendor session can still be
   * re-entered. `null` when none is. A case with nowhere to return to is a verdict being
   * waited on: the step has nothing to offer but a sentence.
   */
  runningCase: { resumable: boolean } | null;
  /**
   * The balance total as the hub reports it, every term included — available, escrowed in
   * orders, invested at NAV, pending withdrawal. "Funded" means money reached the account,
   * not that any of it is still idle.
   */
  total: number;
  /** How many allocations the caller holds units of. Generic over allocations (#245). */
  positions: number;
}

export type StepState = "done" | "current" | "locked";

/** The verify step alone has a fourth state: submitted, decided by someone else, no action. */
export type VerifyState = StepState | "review";

export interface Checklist {
  verify: VerifyState;
  deposit: StepState;
  invest: StepState;
  /** How many of the three are done. */
  done: number;
  /** Every step is done — the block has nothing left to ask for. */
  complete: boolean;
}

export const STEP_COUNT = 3;

/**
 * "Done" always wins over "locked": a tier-0 account that an operator credited by hand has
 * a funded deposit step even though the one before it is still open. The path is the
 * common order, not a rule the hub enforces.
 */
export function deriveChecklist({ level, runningCase, total, positions }: ChecklistRead): Checklist {
  const verified = level > 0;
  const funded = total > 0;
  const invested = positions > 0;

  const verify: VerifyState = verified ? "done" : runningCase !== null && !runningCase.resumable ? "review" : "current";
  const deposit: StepState = funded ? "done" : verified ? "current" : "locked";
  const invest: StepState = invested ? "done" : funded ? "current" : "locked";

  const done = [verify, deposit, invest].filter((s) => s === "done").length;
  return { verify, deposit, invest, done, complete: done === STEP_COUNT };
}
