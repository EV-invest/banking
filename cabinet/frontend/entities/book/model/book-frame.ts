// The socket's reducer: what one frame does to the stream's state, and what the store has
// to go and do about it. Pure and React-free — the two rules worth pinning are that a
// replayed snapshot never overwrites a newer one, and that the caller's orders are
// re-read exactly when `orders_revision` moves, not on every frame.
//
// Only type imports, so `node --test` can load it without resolving the `@/` alias.

import type { BookSnapshot, BookSocketFrame, Trade } from "@/shared/contracts/book";

export interface StreamState {
  /** The highest book revision applied. */
  bookRevision: bigint;
  /** The `orders_revision` last seen. */
  ordersRevision: bigint;
  /** A frame has been applied since this state was created. The FIRST frame after a
   *  mount sets the baseline without asking for a refetch — the screen has just read its
   *  orders on mount, so a refetch there would be the same GET twice. */
  primed: boolean;
}

export const INITIAL_STREAM_STATE: StreamState = { bookRevision: 0n, ordersRevision: 0n, primed: false };

/** What the store does with a frame. `null` means "leave the cache as it is". */
export interface FrameEffects {
  snapshot: BookSnapshot | null;
  /** Newest first, as the wire sends them. */
  trades: Trade[] | null;
  refetchOrders: boolean;
}

const NO_EFFECTS: FrameEffects = { snapshot: null, trades: null, refetchOrders: false };

/** An int64 as the wire spells it — `number | string` — as a bigint; anything else is 0. */
export function revisionOf(value: number | string | undefined): bigint {
  if (value === undefined) return 0n;
  try {
    return BigInt(value);
  } catch {
    return 0n;
  }
}

/**
 * `event.data` → a frame, or `null` for anything that is not our shape. A frame we cannot
 * read is not a reason to tear down a working socket, and never a reason to render.
 */
export function parseFrame(raw: unknown): BookSocketFrame | null {
  if (typeof raw !== "string") return null;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof value !== "object" || value === null) return null;
  const type = (value as { type?: unknown }).type;
  if (type === "book" || type === "heartbeat") return value as BookSocketFrame;
  return null;
}

export function applyFrame(state: StreamState, frame: BookSocketFrame): { state: StreamState; effects: FrameEffects } {
  // Liveness only — the socket already proved that by delivering it.
  if (frame.type !== "book") return { state, effects: NO_EFFECTS };

  const next: StreamState = { ...state, primed: true };
  const effects: FrameEffects = { ...NO_EFFECTS };

  if (frame.snapshot) {
    const revision = revisionOf(frame.snapshot.revision);
    // Only forward movement paints. A replayed or re-ordered frame carries a book we have
    // already shown, and painting it would flicker the levels backwards. Revision 0 is
    // "the hub has not numbered this book yet" and is always taken.
    if (revision === 0n || revision >= state.bookRevision) {
      next.bookRevision = revision;
      effects.snapshot = frame.snapshot;
      effects.trades = frame.trades ?? null;
    }
  }

  const ordersRevision = revisionOf(frame.orders_revision);
  if (ordersRevision !== state.ordersRevision) {
    next.ordersRevision = ordersRevision;
    // The first frame is the baseline: the screen read its orders on mount, and this is
    // the revision that read reflects. Every later move is real.
    effects.refetchOrders = state.primed;
  }

  return { state: next, effects };
}
