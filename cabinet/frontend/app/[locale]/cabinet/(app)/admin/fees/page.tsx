import { I18nScope } from "@evinvest/i18n/react";

import { FeesView } from "@/views/admin/fees/ui/fees-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — fee terms per fund, and settling what they have earned. Authorized
// server-side: the BFF admits only the `admin` and `owner` roles to `/api/admin/fees/*`
// (an operator gets 403, which the view renders as its own state — banking#269), and the
// money plane re-checks `AllocationManage` behind it.
export default async function AdminFeesPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/fees/page.tsx")}>
      <FeesView />
    </I18nScope>
  );
}
