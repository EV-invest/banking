"use client";

// One product's book, kept current.
//
// Sibling in shape to `entities/governance/model/consilium-socket.ts` — module-scoped
// state, subscribers, `useSyncExternalStore`, one hook owning the lifecycle — but keyed
// per service, since two products are two feeds, and different in what a frame is allowed
// to do. The governance frame is a doorbell; this one IS the book: the BFF sends the same
// snapshot `GET /api/book` answers, so a frame is written straight into the read cache
// (`./book-resource`) and the terminal paints from there. The reducer that decides what a
// frame may write is `./book-frame`, and it is where the rules are tested.
//
// ── How it degrades ───────────────────────────────────────────────────────────
//
//   1000  the feed ended        → reconnect at once; the first frame is a full snapshot
//   4401  the access token lapsed → rotate the session; the keeper moves to /login if it
//                                  is genuinely gone, otherwise reconnect with the fresh cookie
//   4503  the hub has no feed   → POLL the two REST reads at POLL_MS and retry the socket
//                                  on the slowest cadence; the page says "polling"
//   else  a blip                → poll underneath, reconnect with backoff
//
// Correctness never depends on the socket: the same snapshot and tape are what the poll
// re-reads, and the only thing lost while it is down is latency. Own orders are never on
// the wire at all — `orders_revision` only says when to re-issue that REST read, and the
// poll re-issues it on its own slower beat.

import { useCallback, useEffect, useSyncExternalStore } from "react";

import { INITIAL_STREAM_STATE, applyFrame, parseFrame, type StreamState } from "@/entities/book/model/book-frame";
import { BOOK_DEPTH, bookSnapshotResource, bookTradesResource, refreshOwnOrders } from "@/entities/book/model/book-resource";
import { apiPath } from "@/shared/config/base-path";
import { refreshSession } from "@/shared/lib/session";

/** The fallback cadence while the socket is down. */
const POLL_MS = 2_000;
/** Own orders are polled every Nth tick — they move on fills, not on every quote. */
const ORDERS_EVERY_TICKS = 5;
const BACKOFF_MIN_MS = 1_000;
const BACKOFF_MAX_MS = 30_000;
const MAX_ATTEMPTS = 5;

const CLOSE_ENDED = 1000;
const CLOSE_EXPIRED = 4401;
const CLOSE_UNAVAILABLE = 4503;
// "No", rather than "not right now": the handshake's policy close and the forbidden echo.
const POLICY_CLOSE_CODES: ReadonlySet<number> = new Set([1008, 4403]);

export type BookStreamStatus =
  /** Nothing is mounted, or this is the server render. */
  | "idle"
  /** The socket is open; the book paints as it moves. */
  | "live"
  /** No socket; the REST reads are being polled. Transient — a reconnect is scheduled. */
  | "reconnecting"
  /** The hub said it cannot serve the feed (4503). Polling, retrying the socket slowly. */
  | "polling"
  /** The tab is in the background: no socket, no polling. */
  | "paused";

export interface BookStream {
  status: BookStreamStatus;
}

interface Channel {
  readonly service: string;
  mounted: number;
  status: BookStreamStatus;
  state: StreamState;
  socket: WebSocket | null;
  attempts: number;
  reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  pollTimer: ReturnType<typeof setInterval> | undefined;
  ticks: number;
  readonly subscribers: Set<() => void>;
  /** Referentially stable between changes — `useSyncExternalStore` compares by identity. */
  snapshot: BookStream;
}

const channels = new Map<string, Channel>();
const SERVER_SNAPSHOT: BookStream = Object.freeze({ status: "idle" });

function channelFor(service: string): Channel {
  const existing = channels.get(service);
  if (existing) return existing;
  const channel: Channel = {
    service,
    mounted: 0,
    status: "idle",
    state: INITIAL_STREAM_STATE,
    socket: null,
    attempts: 0,
    reconnectTimer: undefined,
    pollTimer: undefined,
    ticks: 0,
    subscribers: new Set(),
    snapshot: SERVER_SNAPSHOT,
  };
  channels.set(service, channel);
  return channel;
}

function setStatus(ch: Channel, status: BookStreamStatus): void {
  if (ch.status === status) return;
  ch.status = status;
  ch.snapshot = { status };
  for (const fn of ch.subscribers) fn();
}

function visible(): boolean {
  return typeof document === "undefined" || document.visibilityState === "visible";
}

// ── The fallback ──────────────────────────────────────────────────────────────

function pollOnce(ch: Channel): void {
  // `invalidate` re-reads only what a screen is showing, so a poll costs nothing for a
  // channel whose terminal has navigated away but not yet released.
  bookSnapshotResource.invalidate(ch.service);
  bookTradesResource.invalidate(ch.service);
  ch.ticks += 1;
  if (ch.ticks % ORDERS_EVERY_TICKS === 0) refreshOwnOrders();
}

function startPolling(ch: Channel): void {
  if (ch.pollTimer !== undefined) return;
  ch.pollTimer = setInterval(() => {
    if (visible()) pollOnce(ch);
  }, POLL_MS);
}

function stopPolling(ch: Channel): void {
  if (ch.pollTimer === undefined) return;
  clearInterval(ch.pollTimer);
  ch.pollTimer = undefined;
}

// ── The socket ────────────────────────────────────────────────────────────────

function socketUrl(service: string): string {
  // Same origin as the page, other scheme — the BFF is reached through the zone's own
  // `/api/*` rewrite (the CSP names the origin for this; see `shared/config/security.ts`).
  const url = new URL(apiPath("/api/book/ws"), window.location.href);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.searchParams.set("service", service);
  url.searchParams.set("depth", String(BOOK_DEPTH));
  return url.toString();
}

function onFrame(ch: Channel, event: MessageEvent): void {
  const frame = parseFrame(event.data);
  if (!frame) return;
  const { state, effects } = applyFrame(ch.state, frame);
  ch.state = state;
  if (effects.snapshot) bookSnapshotResource.publish(effects.snapshot, ch.service);
  if (effects.trades) bookTradesResource.publish({ trades: effects.trades }, ch.service);
  if (effects.refetchOrders) refreshOwnOrders();
}

function scheduleReconnect(ch: Channel): void {
  if (ch.reconnectTimer !== undefined || ch.mounted === 0 || !visible()) return;
  // Exponential, capped, jittered — a BFF restart must not bring every open terminal back
  // in the same millisecond. The exponent is clamped as well as the delay.
  const base = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** Math.min(ch.attempts, MAX_ATTEMPTS));
  ch.attempts = Math.min(ch.attempts + 1, MAX_ATTEMPTS);
  ch.reconnectTimer = setTimeout(() => {
    ch.reconnectTimer = undefined;
    connect(ch);
  }, base * (0.5 + Math.random() / 2));
}

/** What a close code means for the next attempt. Returns the status to report meanwhile. */
function onClosed(ch: Channel, code: number): BookStreamStatus {
  if (code === CLOSE_ENDED) {
    // The stream ran out on the server side, not a failure: come straight back.
    ch.attempts = 0;
    return "reconnecting";
  }
  if (code === CLOSE_UNAVAILABLE) {
    ch.attempts = MAX_ATTEMPTS;
    return "polling";
  }
  if (POLICY_CLOSE_CODES.has(code)) ch.attempts = MAX_ATTEMPTS;
  if (code === CLOSE_EXPIRED) {
    // The handshake verifies `ev_access` as the REST routes do. Rotating the session is
    // what re-sets that cookie; if the shell answers "genuinely signed out", the session
    // keeper (`application/layout/session-keeper.tsx`) is what moves to /login — this
    // store never navigates on its own. An unknown answer falls back to the backoff.
    void refreshSession().then((session) => {
      if (session?.authenticated) ch.attempts = 0;
    });
  }
  return "reconnecting";
}

function connect(ch: Channel): void {
  if (ch.mounted === 0 || ch.socket !== null || !visible() || typeof WebSocket === "undefined") return;

  let ws: WebSocket;
  try {
    ws = new WebSocket(socketUrl(ch.service));
  } catch {
    // Blocked outright (a CSP that does not name the origin). The poll is already running.
    scheduleReconnect(ch);
    return;
  }
  ch.socket = ws;

  ws.onopen = () => {
    if (ch.socket !== ws) return;
    ch.attempts = 0;
    // The first frame of a subscription is a full snapshot, so there is nothing to catch
    // up on by hand — and the poll stops: two writers into the same cache entry would
    // race a REST answer against a newer frame.
    stopPolling(ch);
    setStatus(ch, "live");
  };

  ws.onmessage = (event) => {
    if (ch.socket === ws) onFrame(ch, event);
  };

  // Teardown hangs off `onclose` alone: `error` fires first and carries no code, and the
  // code is the whole signal here (see the consilium socket for the full argument).
  ws.onerror = null;
  ws.onclose = (event: CloseEvent) => {
    if (ch.socket !== ws) return;
    ch.socket = null;
    if (ch.mounted === 0) return;
    if (!visible()) {
      stopPolling(ch);
      setStatus(ch, "paused");
      return;
    }
    const status = onClosed(ch, event.code);
    setStatus(ch, status);
    startPolling(ch);
    scheduleReconnect(ch);
  };
}

function closeSocket(ch: Channel): void {
  const ws = ch.socket;
  ch.socket = null;
  if (!ws) return;
  // Detach first: `close()` fires `onclose`, which would otherwise schedule a reconnect.
  ws.onopen = null;
  ws.onmessage = null;
  ws.onerror = null;
  ws.onclose = null;
  try {
    ws.close();
  } catch {
    /* already closing */
  }
}

function clearReconnect(ch: Channel): void {
  if (ch.reconnectTimer === undefined) return;
  clearTimeout(ch.reconnectTimer);
  ch.reconnectTimer = undefined;
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

function wake(ch: Channel): void {
  ch.attempts = 0;
  setStatus(ch, "reconnecting");
  // A backgrounded tab's timers are throttled to nothing, so the book is at its most
  // stale exactly here. One read now, then the poll floor, then the socket.
  pollOnce(ch);
  startPolling(ch);
  connect(ch);
}

function sleep(ch: Channel): void {
  clearReconnect(ch);
  closeSocket(ch);
  stopPolling(ch);
  setStatus(ch, "paused");
}

let watchingVisibility = false;

function onVisibilityChange(): void {
  for (const ch of channels.values()) {
    if (ch.mounted === 0) continue;
    if (visible()) wake(ch);
    else sleep(ch);
  }
}

function acquire(service: string): void {
  const ch = channelFor(service);
  ch.mounted += 1;
  if (ch.mounted > 1) return;
  if (!watchingVisibility) {
    watchingVisibility = true;
    document.addEventListener("visibilitychange", onVisibilityChange);
  }
  if (visible()) wake(ch);
  else setStatus(ch, "paused");
}

function release(service: string): void {
  const ch = channels.get(service);
  if (!ch) return;
  ch.mounted = Math.max(0, ch.mounted - 1);
  if (ch.mounted > 0) return;
  clearReconnect(ch);
  closeSocket(ch);
  stopPolling(ch);
  ch.attempts = 0;
  ch.ticks = 0;
  // Reset with everything else: a retained high-water mark would make the next mount
  // drop every frame of a feed the BFF restarted at a lower revision.
  ch.state = INITIAL_STREAM_STATE;
  setStatus(ch, "idle");
}

/**
 * Subscribe this screen to one product's book, and report how it is being kept current.
 *
 * Reference-counted per service: two surfaces reading the same book share one socket and
 * one poll, and the last to unmount closes them. The data itself is read through
 * `bookSnapshotResource` / `bookTradesResource` — this hook only keeps them fed.
 */
export function useBookStream(service: string, enabled = true): BookStream {
  const active = enabled && service.trim().length > 0;
  useEffect(() => {
    if (!active) return;
    acquire(service);
    return () => release(service);
  }, [active, service]);

  // Stable per service: `useSyncExternalStore` re-subscribes whenever this changes.
  const subscribe = useCallback(
    (onChange: () => void) => {
      const ch = channelFor(service);
      ch.subscribers.add(onChange);
      return () => {
        ch.subscribers.delete(onChange);
      };
    },
    [service],
  );
  const read = useCallback(() => channelFor(service).snapshot, [service]);

  return useSyncExternalStore(subscribe, read, () => SERVER_SNAPSHOT);
}
