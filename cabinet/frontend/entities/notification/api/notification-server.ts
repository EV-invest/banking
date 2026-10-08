import "server-only";

import { bffRead, type ServerRead } from "@/shared/api/server/bff";
import { isJsonObject } from "@/shared/api/server/shape";
import type { NotificationList, NotificationSettings } from "@/shared/contracts/notifications";

// Inbox reads for server components — the endpoints `notification-client.ts` reads, so each
// answer seeds its resource. These shapes are hand-written, not generated, and the views
// map over the lists without a fallback, so the guards insist on the arrays.

function isNotificationList(body: unknown): body is NotificationList {
  return isJsonObject(body) && Array.isArray(body.notifications);
}

function isNotificationSettings(body: unknown): body is NotificationSettings {
  return isJsonObject(body) && Array.isArray(body.topics);
}

/** The first page of the unfiltered inbox — the page `/notifications` opens on. */
export function readNotifications(): Promise<ServerRead<NotificationList> | null> {
  return bffRead("/api/notifications", isNotificationList);
}

export function readNotificationSettings(): Promise<ServerRead<NotificationSettings> | null> {
  return bffRead("/api/notifications/settings", isNotificationSettings);
}
