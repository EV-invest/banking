import { I18nScope } from "@evinvest/i18n/react";

import { CabinetView } from "@/views/admin/cabinet/ui/cabinet-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — cabinet host shell (MFE registry, flags, maintenance). Authorized server-side.
export default async function AdminCabinetPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/cabinet/page.tsx")}>
      <CabinetView />
    </I18nScope>
  );
}
