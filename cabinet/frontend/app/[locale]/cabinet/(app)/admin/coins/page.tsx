import { I18nScope } from "@evinvest/i18n/react";

import { CoinsView } from "@/views/admin/coins/ui/coins-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

export default async function AdminCoinsPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/coins/page.tsx")}>
      <CoinsView />
    </I18nScope>
  );
}
