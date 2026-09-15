/**
 * Whether this browser has put the home banner away, and until when.
 *
 * Per browser, not per account: the dismissal is a reading preference, not a fact about the
 * user, and the cabinet has no field to keep it in — putting one on the profile would mean a
 * migration and a write on every dismissal for something a reload may undo anyway.
 *
 * It expires. A banner dismissed forever is a banner that silently stops onboarding anyone
 * who once shrugged it off, and verification is the gate on the whole product; seven days is
 * long enough that it is not nagging and short enough that it is not abandonment.
 *
 * Every access is wrapped: `localStorage` throws outright in a Safari private window and in a
 * browser with site data blocked, and a banner is not worth a blank page.
 */
const KEY = "ev.kyc.bannerDismissedUntil";

export const DISMISS_MS = 7 * 24 * 60 * 60 * 1000;

export function isBannerDismissed(now: number = Date.now()): boolean {
  const until = read();
  if (until === null) return false;
  if (until > now) return true;
  // Expired: drop it rather than leaving a stale key to be re-read on every mount.
  remove();
  return false;
}

export function dismissBanner(now: number = Date.now()): void {
  write(String(now + DISMISS_MS));
  snapshot = true;
  for (const listener of listeners) listener();
}

// ── The React face of the same value ───────────────────────────────────
//
// An external store rather than state hydrated in an effect: `localStorage` cannot be read
// during a server render, and a component that corrected itself afterwards would paint the
// banner on every load and then take it away in front of a reader who had already dismissed
// it. `useSyncExternalStore` is given a server snapshot instead, and React reconciles the two.

let snapshot: boolean | null = null;
const listeners = new Set<() => void>();

export function subscribeBannerDismissal(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Computed once per page load and then cached: the snapshot has to be referentially stable
 * across renders, and the window it answers about is seven days wide — nothing that can lapse
 * between two paints of the same page.
 */
export function bannerDismissedSnapshot(): boolean {
  if (snapshot === null) snapshot = isBannerDismissed();
  return snapshot;
}

/** The server knows nothing about this browser's choice, so it renders the quieter page. */
export function bannerDismissedServerSnapshot(): boolean {
  return true;
}

function read(): number | null {
  try {
    const raw = window.localStorage.getItem(KEY);
    if (raw === null) return null;
    const until = Number(raw);
    // A hand-edited or half-written value is treated as no value at all, not as "forever".
    return Number.isFinite(until) ? until : null;
  } catch {
    return null;
  }
}

function write(value: string): void {
  try {
    window.localStorage.setItem(KEY, value);
  } catch {
    // Storage is full, blocked, or unavailable. The banner stays dismissed for this page's
    // life through React state either way; it simply comes back on the next load.
  }
}

function remove(): void {
  try {
    window.localStorage.removeItem(KEY);
  } catch {
    // See `write`.
  }
}
