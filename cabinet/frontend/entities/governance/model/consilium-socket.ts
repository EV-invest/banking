"use client";

// The owners' room, kept current.
//
// Sibling in shape to `entities/notification/model/notification-store.ts`: module-scoped
// state, a `Set` of subscribers, `useSyncExternalStore` to read it, and one hook that owns
// the lifecycle. The difference is what it is subscribed to — that store polls because
// there was no stream to listen to; this one has a stream, and keeps the polling as the
// floor underneath it.
//
// ── What a frame is allowed to do ─────────────────────────────────────────────
//
// A frame carries `{ revision, at, heartbeat }` and NOTHING else — no tally, no vote, no
// email address. When the revision moves, this store invalidates the governance reads and
// the existing cache re-fetches the authoritative snapshot over REST. Nothing on screen is
// ever rendered from a frame.
//
// That is not a stylistic preference. The tally is computed by the money plane inside one
// transaction with the request row locked (docs/CONSILIUM.md, policy 14); a count assembled
// anywhere else is a guess about a decision that moves money. Treating the socket as a
// doorbell rather than a delivery also makes its failure modes uninteresting: a duplicated
// frame causes a redundant re-read, a stale one is ignored by the revision check, and a
// spoofed one can at worst make the client ask the server a question it will answer
// correctly. There is no frame content that can put a wrong number in front of an owner.
//
// ── How it degrades ───────────────────────────────────────────────────────────
//
// Honestly, and in public. The socket is best-effort; the polling is the contract. When the
// socket is not open the store polls on the same visibility-aware cadence the notification
// badge uses, and reports `reconnecting` so the page can say so quietly rather than
// pretending to be live. Correctness never depends on the socket: the same re-read runs
// either way, and the only thing lost while it is down is latency.
//
// A closed socket is also how an expired access token presents (the handshake verifies
// `ev_access` exactly as the REST routes do, and the server closes when it lapses — policy
// 22). Nothing special is done for that case, because nothing needs to be: the polling
// fallback issues ordinary REST reads, and those rotate the cookie in `shared/lib/
// api-client.ts` on their way through, so the next reconnect attempt presents a fresh one.

import { useEffect, useSyncExternalStore } from "react";

import { refreshGovernance } from "@/entities/governance/model/governance-resource";
import { apiPath } from "@/shared/config/base-path";
import type { ConsiliumFrame } from "@/shared/contracts/governance";

/** How often the fallback re-reads while the socket is down. */
const POLL_MS = 20_000;
/** First reconnect delay; doubles per consecutive failure up to the cap. */
const BACKOFF_MIN_MS = 1_000;
const BACKOFF_MAX_MS = 30_000;
/** The exponent ceiling — reached, retries sit at BACKOFF_MAX_MS apart and stay there. */
const MAX_ATTEMPTS = 5;

// Closes that mean "no", rather than "not right now": 1008 is the policy violation the
// handshake raises when `ev_access` does not verify, and the 4401/4403 pair is the
// application echo of the HTTP statuses. Retrying these fast changes nothing.
const POLICY_CLOSE_CODES: ReadonlySet<number> = new Set([1008, 4401, 4403]);


export type StreamStatus =
  /** Nothing is mounted, or this is the server render. */
  | "idle"
  /** The socket is open; updates arrive as they happen. */
  | "live"
  /** No socket. The page is still correct — the fallback poll is covering it. */
  | "reconnecting"
  /** The tab is in the background: no socket, no polling, nothing burning battery. */
  | "paused";

export interface ConsiliumStream {
  status: StreamStatus;
  /**
   * The highest revision seen. Exposed for diagnosis and for a "something moved" cue —
   * never as a count of anything. There is no count on this wire to expose.
   */
  revision: number;
}

let status: StreamStatus = "idle";
let revision = 0;
let mounted = 0;
let socket: WebSocket | null = null;
let attempts = 0;
let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
let pollTimer: ReturnType<typeof setInterval> | undefined;

const subscribers = new Set<() => void>();

// Rebuilt only when something changes: `useSyncExternalStore` compares snapshots by
// identity, and a fresh object per read would re-render every subscriber forever.
let snapshot: ConsiliumStream = { status, revision };
const SERVER_SNAPSHOT: ConsiliumStream = Object.freeze({ status: "idle", revision: 0 });

function publish(): void {
  if (snapshot.status === status && snapshot.revision === revision) return;
  snapshot = { status, revision };
  for (const fn of subscribers) fn();
}

function subscribe(fn: () => void): () => void {
  subscribers.add(fn);
  return () => {
    subscribers.delete(fn);
  };
}

function visible(): boolean {
  return typeof document === "undefined" || document.visibilityState === "visible";
}

// ── The fallback ──────────────────────────────────────────────────────────────

function startPolling(): void {
  if (pollTimer !== undefined) return;
  pollTimer = setInterval(() => {
    if (visible()) refreshGovernance();
  }, POLL_MS);
}

function stopPolling(): void {
  if (pollTimer === undefined) return;
  clearInterval(pollTimer);
  pollTimer = undefined;
}

// ── The socket ────────────────────────────────────────────────────────────────

function socketUrl(): string {
  // Same origin as the page — the BFF is reached through the zone's own `/api/*` rewrite,
  // so there is no second host here, only a second scheme. (That scheme is why the CSP
  // names the origin explicitly; see `shared/config/security.ts`.)
  const url = new URL(apiPath("/api/owners/consilium/ws"), window.location.href);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}

function onMessage(event: MessageEvent): void {
  let frame: ConsiliumFrame;
  try {
    frame = JSON.parse(String(event.data)) as ConsiliumFrame;
  } catch {
    // Not our shape. A frame we cannot read is not a reason to tear down a working socket,
    // and it is certainly not a reason to render anything.
    return;
  }

  if (frame.heartbeat) return; // Liveness only — the socket already proved that by arriving.

  const next = frame.revision;
  if (typeof next !== "number" || !Number.isFinite(next)) return;
  // Only forward movement counts. A replayed or re-ordered frame carries a revision we have
  // already acted on, and acting on it again would be a re-fetch for nothing.
  if (next <= revision) return;

  revision = next;
  publish();
  // The frame said "something moved". This is what asks what it was.
  refreshGovernance();
}

function scheduleReconnect(): void {
  if (reconnectTimer !== undefined || mounted === 0 || !visible()) return;
  // Exponential, capped, with jitter so a BFF restart does not bring every open cabinet
  // back in the same millisecond. The exponent is clamped as well as the delay, so a tab
  // left open for a day cannot overflow it into an infinity.
  const base = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** Math.min(attempts, MAX_ATTEMPTS));
  attempts = Math.min(attempts + 1, MAX_ATTEMPTS);
  reconnectTimer = setTimeout(() => {
    reconnectTimer = undefined;
    connect();
  }, base * (0.5 + Math.random() / 2));
}

function connect(): void {
  if (mounted === 0 || socket !== null || !visible() || typeof WebSocket === "undefined") return;

  let ws: WebSocket;
  try {
    ws = new WebSocket(socketUrl());
  } catch {
    // Blocked outright (a CSP that does not name the origin, a hostile extension). The
    // fallback is already running; retry on the same backoff rather than giving up.
    scheduleReconnect();
    return;
  }
  socket = ws;

  ws.onopen = () => {
    if (socket !== ws) return;
    attempts = 0;
    status = "live";
    publish();
    // The gap between losing the socket and regaining it is exactly when the room moved
    // without telling us. Catch up before trusting the stream again.
    refreshGovernance();
    // Deliberately kept running. A socket that is open but silently dead (a proxy holding
    // the connection open, a laptop resumed from sleep) is indistinguishable from a quiet
    // room, and the poll is what makes that case merely slow instead of wrong.
    startPolling();
  };

  ws.onmessage = onMessage;

  const down = () => {
    if (socket !== ws) return;
    socket = null;
    if (mounted === 0) return;
    if (!visible()) {
      // The visibility handler owns a hidden tab. Starting a poll here would leave a 20s
      // interval running in a tab this store is reporting as "paused" — burning the
      // battery it exists to save, and contradicting its own status.
      stopPolling();
      status = "paused";
      publish();
      return;
    }
    status = "reconnecting";
    publish();
    startPolling();
    scheduleReconnect();
  };

  // Teardown hangs off `onclose` ALONE, and deliberately not off `onerror` too.
  //
  // Both fire for a failed connection, and `error` comes first — but `error` carries no
  // close code. Tearing down there nulls the socket, so by the time `close` arrives with
  // the code, the handler has nothing left to attribute it to: a policy close preceded by
  // an error (the normal ordering) would never be recognised, and the store would keep
  // retrying a settled "no" every second. The spec guarantees a `close` after an `error`
  // on a failed connection, so `close` is the complete signal and the earlier one is only
  // a notification.
  //
  // A close the server chose — an expired or revoked `ev_access` (policy 22), or a caller
  // who is not an owner — will keep happening. Backing off to the slowest cadence lets the
  // fallback poll carry the page instead; those REST reads rotate the access cookie on
  // their way through, so a later attempt at that cadence is the one that can succeed.
  ws.onerror = null;
  ws.onclose = (event: CloseEvent) => {
    if (socket === ws && POLICY_CLOSE_CODES.has(event.code)) attempts = MAX_ATTEMPTS;
    down();
  };
}

function closeSocket(): void {
  const ws = socket;
  socket = null;
  if (!ws) return;
  // Detach first: `close()` fires `onclose`, and the handler would otherwise schedule a
  // reconnect for a socket we are deliberately abandoning.
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

function clearReconnect(): void {
  if (reconnectTimer === undefined) return;
  clearTimeout(reconnectTimer);
  reconnectTimer = undefined;
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

function onVisibilityChange(): void {
  if (mounted === 0) return;
  if (visible()) {
    attempts = 0;
    status = "reconnecting";
    publish();
    // A backgrounded tab's timers are throttled to nothing, so the room is at its most
    // stale precisely here. Re-read before the socket has even finished opening.
    refreshGovernance();
    startPolling();
    connect();
  } else {
    // Hidden: hold nothing open. The socket costs the server a connection and the client a
    // wake-up, and there is nobody looking at the result.
    clearReconnect();
    closeSocket();
    stopPolling();
    status = "paused";
    publish();
  }
}

function acquire(): void {
  mounted += 1;
  if (mounted > 1) return;
  document.addEventListener("visibilitychange", onVisibilityChange);
  if (!visible()) {
    status = "paused";
    publish();
    return;
  }
  status = "reconnecting";
  publish();
  startPolling();
  connect();
}

function release(): void {
  mounted = Math.max(0, mounted - 1);
  if (mounted > 0) return;
  document.removeEventListener("visibilitychange", onVisibilityChange);
  clearReconnect();
  closeSocket();
  stopPolling();
  attempts = 0;
  // Reset with everything else. This is module-global and outlives the page: a BFF redeploy
  // (or simply a different room) can restart the counter lower, and a retained high-water
  // mark would make `onMessage` drop every frame from then on — the page would sit on the
  // fallback poll forever while still reporting itself live.
  revision = 0;
  status = "idle";
  publish();
}

/**
 * Subscribe this screen to the owners' room, and report how it is being kept current.
 *
 * Reference-counted, so two surfaces reading it share one socket and one poll, and the last
 * one to unmount is what closes them. Nothing is left behind: no socket, no interval, no
 * reconnect timer, no listener.
 */
export function useConsiliumStream(enabled = true): ConsiliumStream {
  useEffect(() => {
    if (!enabled) return;
    acquire();
    return release;
  }, [enabled]);

  return useSyncExternalStore(
    subscribe,
    () => snapshot,
    () => SERVER_SNAPSHOT,
  );
}
