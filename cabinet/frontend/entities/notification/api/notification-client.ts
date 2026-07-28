import { apiPath } from "@/shared/config/base-path";
import { csrfHeader } from "@/shared/lib/csrf-client";
import type {
	MarkReadResult,
	NotificationList,
	NotificationSettings,
	UnreadCount,
} from "@/shared/contracts/notifications";

async function getJson<T>(url: `/${string}`): Promise<T> {
	const res = await fetch(apiPath(url), { headers: { accept: "application/json" } });
	const data = (await res.json().catch(() => ({}))) as T & { error?: string };
	if (!res.ok) throw new Error(data.error ?? `request failed (${res.status})`);
	return data;
}

async function postJson<T>(url: `/${string}`, body: unknown): Promise<T> {
	const res = await fetch(apiPath(url), {
		method: "POST",
		headers: { "content-type": "application/json", ...csrfHeader() },
		body: JSON.stringify(body),
	});
	const data = (await res.json().catch(() => ({}))) as T & { error?: string };
	if (!res.ok) throw new Error(data.error ?? `request failed (${res.status})`);
	return data;
}

export interface ListParams {
	cursor?: string;
	limit?: number;
	filter?: "all" | "unread";
	topic?: string;
}

export function fetchNotifications(params: ListParams = {}): Promise<NotificationList> {
	const q = new URLSearchParams();
	if (params.cursor) q.set("cursor", params.cursor);
	if (params.limit) q.set("limit", String(params.limit));
	// "all" is the server default, so only the narrowing filter is worth sending.
	if (params.filter === "unread") q.set("filter", "unread");
	if (params.topic) q.set("topic", params.topic);
	const qs = q.toString();
	return getJson<NotificationList>(`/api/notifications${qs ? `?${qs}` : ""}`);
}

export function fetchUnreadCount(): Promise<UnreadCount> {
	return getJson<UnreadCount>("/api/notifications/unread-count");
}

/** Mark specific ids read. Pass nothing to mark everything read. */
export function markRead(ids?: string[]): Promise<MarkReadResult> {
	return postJson<MarkReadResult>("/api/notifications/read", ids?.length ? { ids } : { all: true });
}

export function fetchNotificationSettings(): Promise<NotificationSettings> {
	return getJson<NotificationSettings>("/api/notifications/settings");
}

/**
 * Flip a master channel. Both channels may end up off — the caller must not
 * "helpfully" re-enable the other one.
 */
export function setChannelEnabled(channel: "in_app" | "email", enabled: boolean): Promise<NotificationSettings> {
	return postJson<NotificationSettings>("/api/notifications/settings/channel", { channel, enabled });
}

/**
 * Follow or unfollow a topic. `emailEnabled` defaults to true server-side, so a
 * one-click "follow" from a product page opts into the email copy unless told not to.
 */
export function setTopicSubscription(
	topic: string,
	subscribed: boolean,
	emailEnabled = true,
): Promise<NotificationSettings> {
	return postJson<NotificationSettings>("/api/notifications/settings/topic", {
		topic,
		subscribed,
		email_enabled: emailEnabled,
	});
}
