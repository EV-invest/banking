/**
 * What this browser remembers about the checklist: nothing while a step is open, and one of
 * two facts once it has been seen.
 *
 *   · `open` — the block was on screen with a step still to do. A later "all done" is then a
 *     transition this browser watched, which is what earns the one-time "you are set" line.
 *     A fresh browser on an account that finished long ago has no such before, and a line
 *     that congratulated every returning investor would be noise, not news. (Same reasoning as
 *     `features/kyc/lib/kyc-completion`: completion is a transition, and a transition needs a
 *     before.)
 *   · `acknowledged` — the reader put that line away. From here the block is a one-line
 *     status and asks for nothing.
 *
 * A browser that remembers neither renders nothing for a finished path: a veteran opening a
 * fresh browser never saw the checklist, and a status line for a path they never walked is
 * an explanation of nothing. The performance card simply stays first.
 *
 * Per browser, not per account: this is a reading preference, not a fact about the user, and
 * the cabinet has no field to keep it in — a profile column would mean a migration and a write
 * for something a reload may undo anyway (#382). The block itself is NOT dismissible while a
 * step is open, which is what makes per-browser acceptable: forgetting costs a repeat of a
 * line, never a lost onboarding.
 *
 * Every access is wrapped: `localStorage` throws outright in a Safari private window and in a
 * browser with site data blocked, and a status line is not worth a blank page.
 */
const KEY = "ev.onboarding.stage";

export type ChecklistStage = "open" | "acknowledged";

export function readStage(): ChecklistStage | null {
  try {
    const raw = window.localStorage.getItem(KEY);
    // A hand-edited or half-written value is treated as no value at all.
    return raw === "open" || raw === "acknowledged" ? raw : null;
  } catch {
    return null;
  }
}

/** Idempotent, and never demotes: an acknowledged checklist does not reopen. */
export function markOpen(): void {
  if (readStage() !== null) return;
  write("open");
}

export function acknowledge(): void {
  write("acknowledged");
}

/** What a finished path shows in this browser — see the note on the key above. */
export type CompletionView = "all-set" | "line" | null;

export function completionView(stage: ChecklistStage | null): CompletionView {
  if (stage === "open") return "all-set";
  if (stage === "acknowledged") return "line";
  return null;
}

// ── The React face of the same value ───────────────────────────────────
//
// An external store rather than state hydrated in an effect: `localStorage` cannot be read
// during a server render, and a component that corrected itself afterwards would paint one
// state and then swap it in front of the reader. `useSyncExternalStore` is given a server
// snapshot instead, and React reconciles the two.

let snapshot: ChecklistStage | null | undefined;
const listeners = new Set<() => void>();

export function subscribeStage(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Read once per page load and then cached: the snapshot has to be referentially stable. */
export function stageSnapshot(): ChecklistStage | null {
  if (snapshot === undefined) snapshot = readStage();
  return snapshot;
}

/** The server knows nothing about this browser, so it renders the quieter state. */
export function stageServerSnapshot(): ChecklistStage | null {
  return null;
}

function write(stage: ChecklistStage): void {
  try {
    window.localStorage.setItem(KEY, stage);
  } catch {
    // Storage is full, blocked, or unavailable. The page keeps the value for its own life
    // through the snapshot below; it simply starts over on the next load.
  }
  snapshot = stage;
  for (const listener of listeners) listener();
}
