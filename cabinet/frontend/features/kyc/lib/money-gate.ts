/**
 * Should a money surface stand closed for this reader?
 *
 * Three inputs and one answer, written down once because the expression had been copied
 * verbatim into three views — `/wallet`, `/wallet/deposit`, `/wallet/withdraw` — along with
 * the paragraph explaining why a failed read must not gate. A predicate that lives in three
 * places is a predicate with three futures, and this one decides whether someone sees their
 * deposit address.
 *
 * Pure, so it can be run: the cabinet has no component tests, and `node --test` over
 * `*.test.ts` is the only level this rule can be checked at.
 */
export interface MoneyGateRead {
  /** The tier to believe — the identity plane's when it answered, the profile's otherwise. */
  level: number;
  /**
   * Whether that tier came from anywhere at all. `false` means BOTH reads failed, which is
   * not a tier of 0: gating on it would take a verified reader's rails away over an unrelated
   * blip, which is a worse failure than briefly showing rails to someone who cannot use them
   * (the hub refuses those actions itself, and says why).
   */
  settled: boolean;
  /** Nothing has been read yet. The surface owes a skeleton here, not a verdict. */
  loading: boolean;
}

export function isMoneyGated({ level, settled, loading }: MoneyGateRead): boolean {
  return !loading && settled && level === 0;
}
