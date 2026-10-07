import { I18nScope } from "@evinvest/i18n/react";

import { TreasuryView } from "@/views/admin/treasury/ui/treasury-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — treasury (two-layer chart of accounts). Authorized server-side.
export default async function AdminTreasuryPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/treasury/page.tsx")}>
      <TreasuryView />
    </I18nScope>
  );
}
