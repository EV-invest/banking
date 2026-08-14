"use client";

// The inbox reads. Two of them, cached for different reasons.
//
// `notificationsResource` caches the FIRST page per filter — the one every visit to the
// inbox starts from. Later pages are appended by the view and are not cached: a cursor page
// is only meaningful after the pages before it, so caching them separately would buy nothing
// and risk stitching two different points in time together.
//
// `notificationSettingsResource` is the channel/topic matrix Settings edits. Every mutation
// on it answers with the whole new matrix, so the setters publish write-through instead of
// invalidating.
//
// The unread badge is NOT here: it polls on a cadence of its own from every signed-in
// screen and already has a home in `notification-store.ts`.

import { fetchNotificationSettings, fetchNotifications, markRead as markReadRequest, setChannelEnabled as setChannelEnabledRequest, setTopicSubscription as setTopicSubscriptionRequest } from "@/entities/notification/api/notification-client";
import { publishUnreadCount } from "@/entities/notification/model/notification-store";
import type { MarkReadResult, NotificationSettings } from "@/shared/contracts/notifications";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource } from "@/shared/lib/resource";

export type NotificationFilter = "all" | "unread";

export const notificationsResource = defineResource({
  name: "notifications.page",
  fetch: (filter: NotificationFilter) => fetchNotifications({ filter }),
  key: (filter) => filter,
  tags: [TAG.notifications],
});

export const notificationSettingsResource = defineResource({
  name: "notifications.settings",
  fetch: fetchNotificationSettings,
  revalidate: 60,
  tags: [TAG.notifications],
});

/**
 * Mark ids read, or everything when given none.
 *
 * Mark-all invalidates both filters rather than patching: under "unread" every row should
 * leave the list, which no local edit would do. Marking ONE id does not — the inbox patches
 * that row into the cache optimistically, and refetching there would also throw away any
 * older pages the reader had loaded under it.
 */
export async function markRead(ids?: string[]): Promise<MarkReadResult> {
  const result = await markReadRequest(ids);
  publishUnreadCount(result.unread_count);
  if (!ids?.length) notificationsResource.invalidateAll();
  return result;
}

export async function setChannelEnabled(channel: "in_app" | "email", enabled: boolean): Promise<NotificationSettings> {
  const settings = await setChannelEnabledRequest(channel, enabled);
  notificationSettingsResource.publish(settings);
  return settings;
}

export async function setTopicSubscription(topic: string, subscribed: boolean, emailEnabled = true): Promise<NotificationSettings> {
  const settings = await setTopicSubscriptionRequest(topic, subscribed, emailEnabled);
  notificationSettingsResource.publish(settings);
  return settings;
}
