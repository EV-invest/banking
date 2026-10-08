import { I18nScope } from "@evinvest/i18n/react";

import { OutboxView } from "@/views/admin/outbox/ui/outbox-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — parked outbox rows and the unpark action. Fleet health and the relay
// KPIs live in Grafana; this stays because unparking is an action, not a reading.
// Authorized server-side by the BFF admin routes (role-gated); the nav is hidden for
// non-operators.
export default async function AdminOutboxPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/outbox/page.tsx")}>
      <OutboxView />
    </I18nScope>
  );
}
