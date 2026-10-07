import { I18nScope } from "@evinvest/i18n/react";

import { ValuationView } from "@/views/admin/valuation/ui/valuation-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — valuation & redemptions (post NAV, clear the queue). Authorized server-side.
export default async function AdminValuationPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/valuation/page.tsx")}>
      <ValuationView />
    </I18nScope>
  );
}
