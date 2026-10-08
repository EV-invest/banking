import { I18nScope } from "@evinvest/i18n/react";

import { RevenueView } from "@/views/admin/revenue/ui/revenue-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — the platform's earned revenue, the reserved `fee` allocation.
// Authorized server-side; the BFF re-checks the admin role at the money plane.
export default async function AdminRevenuePage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/revenue/page.tsx")}>
      <RevenueView />
    </I18nScope>
  );
}
