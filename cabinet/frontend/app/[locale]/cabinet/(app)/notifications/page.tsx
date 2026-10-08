import { Suspense } from "react";

import { I18nScope } from "@evinvest/i18n/react";

import { readNotifications } from "@/entities/notification/api/notification-server";
import { isClientNavigation } from "@/shared/api/server/navigation";
import { NotificationsView } from "@/views/notifications/ui/notifications-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The inbox, its first unfiltered page read on the server — the /wallet pattern: the
// boundary's fallback is the same view in its loading state, flushed at once, and the read
// streams in behind it. A failed or timed-out read arrives as no seed and the view makes
// the browser read it always made. Filters and older pages stay browser reads, and a
// client-side navigation skips the server read: the browser cache answers those.
export default async function NotificationsPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/notifications/page.tsx")}>
      <Suspense fallback={<NotificationsView />}>
        <SeededNotifications />
      </Suspense>
    </I18nScope>
  );
}

async function SeededNotifications() {
  if (await isClientNavigation()) return <NotificationsView />;
  const initial = (await readNotifications()) ?? undefined;
  return <NotificationsView initial={initial} />;
}
