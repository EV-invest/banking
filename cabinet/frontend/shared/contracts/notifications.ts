// Notification shapes. Hand-written rather than generated, for the same reason
// `platform.ts` and `admin.ts` are: the BFF composes these responses, so they are not
// a 1:1 passthrough of any single proto message.
//
// Timestamps arrive as strings holding unix SECONDS — the BFF renders every i64 that
// way to match the wire shape the generated types expect. Multiply before handing one
// to `Date`.

export type NotificationTopic =
	| "fund:quy-nhon"
	| "fund:coastal-yield"
	| "account:money-movement"
	| "platform:announcements";

export interface Notification {
	id: string;
	topic: string;
	kind: string;
	title: string;
	body: string;
	/** Cabinet-relative path, or "" when the notification links nowhere. */
	link: string;
	occurred_at: string;
	created_at: string;
	/** "0" while unread. */
	read_at: string;
}

export interface NotificationList {
	notifications: Notification[];
	/** "" when there is no further page. */
	next_cursor: string;
	unread_count: number;
}

export interface UnreadCount {
	unread_count: number;
}

export interface MarkReadResult {
	marked: number;
	unread_count: number;
}

export interface TopicSubscription {
	topic: string;
	/** Server-owned copy, so every client renders identical wording. */
	label: string;
	description: string;
	subscribed: boolean;
	email_enabled: boolean;
}

/**
 * The whole delivery surface. `in_app_enabled` and `email_enabled` may BOTH be false —
 * that is a supported "stop contacting me" state, not a broken record, so never treat
 * it as an error or silently force one back on.
 */
export interface NotificationSettings {
	in_app_enabled: boolean;
	email_enabled: boolean;
	email: string;
	email_verified: boolean;
	topics: TopicSubscription[];
}

/** Unread iff `read_at` is absent or the sentinel "0". */
export function isUnread(n: Notification): boolean {
	return !n.read_at || n.read_at === "0";
}

/** Unix seconds (as a string) → `Date`. Returns null for the unread sentinel. */
export function toDate(unixSeconds: string): Date | null {
	const n = Number(unixSeconds);
	return Number.isFinite(n) && n > 0 ? new Date(n * 1000) : null;
}
