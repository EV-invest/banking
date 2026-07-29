"use client";

import { useEffect, useSyncExternalStore } from "react";

import { fetchUnreadCount } from "@/entities/notification/api/notification-client";

/**
 * Module-scoped unread-count store, mirroring `entities/user/model/profile-store.ts`.
 *
 * The badge is read from several places at once (sidebar, page header), so the count
 * lives here rather than in any one component: a `markRead` anywhere updates every
 * subscriber without prop-drilling or a refetch storm.
 *
 * Polling, not streaming. There is no SSE/WS route, and the BFF's 15s request deadline
 * would kill a long-lived stream anyway — so a slow interval is the honest mechanism.
 * The tab-hidden check keeps a backgrounded tab from polling forever.
 */

const POLL_MS = 60_000;

let cached: number | null = null;
let inflight: Promise<void> | null = null;
const subscribers = new Set<() => void>();

function emit() {
	for (const fn of subscribers) fn();
}

function subscribe(fn: () => void): () => void {
	subscribers.add(fn);
	return () => {
		subscribers.delete(fn);
	};
}

/** Overwrite the count from a response that already carried it (no extra request). */
export function publishUnreadCount(count: number) {
	if (cached === count) return;
	cached = count;
	emit();
}

/** Refresh from the server. Concurrent callers share one request. */
export function refreshUnreadCount(): Promise<void> {
	if (inflight) return inflight;
	inflight = fetchUnreadCount()
		.then((r) => publishUnreadCount(r.unread_count))
		// A failed poll must not blank an already-good badge, and must not surface as
		// an error anywhere: this is ambient background state, not a user action.
		.catch(() => {})
		.finally(() => {
			inflight = null;
		});
	return inflight;
}

/** The current count, or null until the first load settles. */
export function useUnreadCount(): number | null {
	return useSyncExternalStore(
		subscribe,
		() => cached,
		() => null,
	);
}

/**
 * Mount once per surface that shows the badge. Fetches immediately, then polls while
 * the tab is visible and refreshes on the way back from a hidden tab.
 */
export function useUnreadCountPolling(enabled = true) {
	useEffect(() => {
		if (!enabled) return;

		let timer: ReturnType<typeof setTimeout> | undefined;

		const tick = () => {
			if (typeof document === "undefined" || document.visibilityState === "visible") {
				void refreshUnreadCount();
			}
			timer = setTimeout(tick, POLL_MS);
		};
		tick();

		const onVisible = () => {
			if (document.visibilityState === "visible") void refreshUnreadCount();
		};
		document.addEventListener("visibilitychange", onVisible);

		return () => {
			if (timer) clearTimeout(timer);
			document.removeEventListener("visibilitychange", onVisible);
		};
	}, [enabled]);
}
