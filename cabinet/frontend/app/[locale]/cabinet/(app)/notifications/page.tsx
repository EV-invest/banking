import { I18nScope } from "@evinvest/i18n/react";

import { NotificationsView } from "@/views/notifications/ui/notifications-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

export default async function NotificationsPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/notifications/page.tsx")}>
      <NotificationsView />
    </I18nScope>
  );
}
