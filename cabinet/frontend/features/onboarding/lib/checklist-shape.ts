/**
 * The shape the checklist block last took in this browser — the card, the one-time all-set
 * card, the status line, or nothing — mirrored into a cookie so the server can draw its
 * skeleton in that shape before any read has answered.
 *
 * Why a cookie. The block's final shape depends on `/kyc/status`, which only the browser can
 * read, and on the per-browser stage in `localStorage` (`./checklist-memory`), which the
 * server cannot see. Without a hint the server's placeholder is a guess, and a wrong guess is
 * the whole page jumping by a card's height on every visit for one kind of account or the
 * other. It is a layout hint and nothing else: never a verdict, never shown as words, and a
 * missing or stale one costs only the jump it was there to prevent.
 *
 * Plain module, no imports: the route reads it on the server, `node --test` can run it.
 */
export type ChecklistShape = "path" | "all-set" | "line" | "none";

export const SHAPE_COOKIE = "ev_checklist_shape";

export function isChecklistShape(value: unknown): value is ChecklistShape {
  return value === "path" || value === "all-set" || value === "line" || value === "none";
}

/** A year: the hint outlives a session the way the stage it mirrors does. */
export function shapeCookie(shape: ChecklistShape): string {
  return `${SHAPE_COOKIE}=${shape}; Path=/; Max-Age=31536000; SameSite=Lax`;
}
